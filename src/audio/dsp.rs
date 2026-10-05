//! Realtime-safe DSP: 10-band peaking equalizer, preamp, volume and balance.
//! Nothing here allocates after construction.

use crate::config::BANDS;

pub const BAND_FREQS: [f32; BANDS] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];
pub const BAND_LABELS: [&str; BANDS] = [
    "60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K",
];
const Q: f64 = 1.2;
const SMOOTH_BLOCK: usize = 64;
const SMOOTH_COEF: f32 = 0.15;
const EPS: f32 = 0.01;

#[derive(Debug, Clone, Copy, Default)]
pub struct Coeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coeffs {
    pub const UNITY: Coeffs = Coeffs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// RBJ cookbook peaking EQ, normalized by a0.
    pub fn peaking(f0: f64, gain_db: f64, q: f64, fs: f64) -> Self {
        let f0 = f0.min(fs * 0.45);
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * f0 / fs;
        let alpha = w0.sin() / (2.0 * q);
        let cw = w0.cos();
        let a0 = 1.0 + alpha / a;
        Coeffs {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cw) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cw) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }

    /// Magnitude response at `f` (used by tests).
    #[cfg(test)]
    pub fn magnitude(&self, f: f64, fs: f64) -> f64 {
        use std::f64::consts::PI;
        let w = 2.0 * PI * f / fs;
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

/// Transposed direct form II biquad with stereo state.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    c: Coeffs,
    z1: [f64; 2],
    z2: [f64; 2],
}

impl Biquad {
    const UNITY: Biquad = Biquad {
        c: Coeffs::UNITY,
        z1: [0.0; 2],
        z2: [0.0; 2],
    };

    #[inline]
    fn process(&mut self, ch: usize, x: f64) -> f64 {
        let c = &self.c;
        let y = c.b0 * x + self.z1[ch];
        self.z1[ch] = c.b1 * x - c.a1 * y + self.z2[ch];
        self.z2[ch] = c.b2 * x - c.a2 * y;
        y
    }

    /// Zeroes state that decayed toward the subnormal range.
    fn flush_tiny(&mut self) {
        for z in self.z1.iter_mut().chain(self.z2.iter_mut()) {
            if z.abs() < 1e-30 {
                *z = 0.0;
            }
        }
    }
}

/// Target parameters, written by the UI through atomics and copied in each callback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqParams {
    pub enabled: bool,
    pub preamp_db: f32,
    pub bands_db: [f32; BANDS],
}

impl Default for EqParams {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            bands_db: [0.0; BANDS],
        }
    }
}

pub struct Equalizer {
    fs: f64,
    filters: [Biquad; BANDS],
    cur_db: [f32; BANDS],
    cur_preamp_db: f32,
    /// Linear gain for `cur_preamp_db`, updated with it.
    pre_gain: f64,
    counter: usize,
}

impl Equalizer {
    pub fn new(fs: f64) -> Self {
        Self {
            fs,
            filters: [Biquad::UNITY; BANDS],
            cur_db: [0.0; BANDS],
            cur_preamp_db: 0.0,
            pre_gain: 1.0,
            counter: 0,
        }
    }

    /// Disabling ramps every band to 0 dB, so toggling never clicks.
    fn targets(p: &EqParams) -> ([f32; BANDS], f32) {
        if p.enabled {
            (p.bands_db, p.preamp_db)
        } else {
            ([0.0; BANDS], 0.0)
        }
    }

    fn smooth(&mut self, p: &EqParams) {
        let (bands, preamp) = Self::targets(p);
        for (i, &target) in bands.iter().enumerate() {
            let cur = self.cur_db[i];
            if cur == target {
                continue;
            }
            let next = cur + (target - cur) * SMOOTH_COEF;
            let next = if (target - next).abs() <= EPS { target } else { next };
            self.cur_db[i] = next;
            let f = &mut self.filters[i];
            if next == 0.0 {
                // Band is bypassed from now on; drop its state so re-enabling starts clean.
                *f = Biquad::UNITY;
            } else {
                f.c = Coeffs::peaking(BAND_FREQS[i] as f64, next as f64, Q, self.fs);
            }
        }
        let d = preamp - self.cur_preamp_db;
        self.cur_preamp_db = if d.abs() > EPS {
            self.cur_preamp_db + d * SMOOTH_COEF
        } else {
            preamp
        };
        self.pre_gain = 10f64.powf(self.cur_preamp_db as f64 / 20.0);
    }

    fn is_flat(&self) -> bool {
        self.cur_preamp_db == 0.0 && self.cur_db.iter().all(|&g| g == 0.0)
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, buf: &mut [f32], p: &EqParams) {
        let (bands, preamp) = Self::targets(p);
        if self.is_flat() && preamp == 0.0 && bands.iter().all(|&g| g == 0.0) {
            return;
        }
        for frame in buf.as_chunks_mut::<2>().0 {
            if self.counter == 0 {
                self.smooth(p);
            }
            self.counter = (self.counter + 1) % SMOOTH_BLOCK;
            let pre = self.pre_gain;
            for (ch, s) in frame.iter_mut().enumerate() {
                let mut x = *s as f64 * pre;
                for (i, f) in self.filters.iter_mut().enumerate() {
                    if self.cur_db[i] != 0.0 {
                        x = f.process(ch, x);
                    }
                }
                *s = x as f32;
            }
        }
        // In silence the state decays into subnormals, where every multiply
        // costs ~100x on x86 exactly when the CPU should idle. Once per block.
        for (f, &db) in self.filters.iter_mut().zip(&self.cur_db) {
            if db != 0.0 {
                f.flush_tiny();
            }
        }
    }
}

/// Linear left/right gains from volume (0..1, squared law) and balance (-1..1).
pub fn channel_gains(volume: f32, balance: f32) -> (f32, f32) {
    let v = volume.clamp(0.0, 1.0);
    let v = v * v;
    let b = balance.clamp(-1.0, 1.0);
    (v * (1.0 - b).min(1.0), v * (1.0 + b).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_gain_is_unity() {
        let c = Coeffs::peaking(1000.0, 0.0, Q, 48000.0);
        for f in [50.0, 1000.0, 10000.0] {
            assert!((c.magnitude(f, 48000.0) - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn peak_gain_at_center() {
        for &g in &[12.0, -12.0, 6.0] {
            let c = Coeffs::peaking(1000.0, g, Q, 48000.0);
            let db = 20.0 * c.magnitude(1000.0, 48000.0).log10();
            assert!((db - g).abs() < 0.01, "{db} vs {g}");
            let far = 20.0 * c.magnitude(20.0, 48000.0).log10();
            assert!(far.abs() < 0.5);
        }
    }

    #[test]
    fn f0_clamped_below_nyquist() {
        let c = Coeffs::peaking(16000.0, 12.0, Q, 22050.0);
        assert!(c.b0.is_finite() && c.a1.is_finite());
    }

    #[test]
    fn equalizer_disabled_passthrough() {
        let mut eq = Equalizer::new(48000.0);
        let mut buf = vec![0.5f32; 256];
        eq.process(&mut buf, &EqParams::default());
        assert!(buf.iter().all(|&s| s == 0.5));
    }

    #[test]
    fn equalizer_converges_and_stays_stable() {
        let mut eq = Equalizer::new(48000.0);
        let p = EqParams {
            enabled: true,
            preamp_db: -6.0,
            bands_db: [12.0; BANDS],
        };
        let mut buf = vec![0.0f32; 48000 * 2];
        for (i, s) in buf.iter_mut().enumerate() {
            *s = ((i / 2) as f32 * 0.05).sin() * 0.1;
        }
        eq.process(&mut buf, &p);
        assert!(buf.iter().all(|s| s.is_finite()));
        assert!((eq.cur_db[0] - 12.0).abs() < 1e-6);
        assert!((eq.cur_preamp_db + 6.0).abs() < 1e-6);
    }

    #[test]
    fn eq_state_flushes_to_zero_in_silence() {
        let mut eq = Equalizer::new(48000.0);
        let p = EqParams {
            enabled: true,
            preamp_db: 0.0,
            bands_db: [12.0; BANDS],
        };
        let mut buf = vec![0.0f32; 24000 * 2];
        for (i, s) in buf.iter_mut().enumerate() {
            *s = ((i / 2) as f32 * 0.05).sin() * 0.5;
        }
        eq.process(&mut buf, &p);
        let state = |eq: &Equalizer| -> Vec<f64> {
            eq.filters
                .iter()
                .flat_map(|f| f.z1.into_iter().chain(f.z2))
                .collect()
        };
        assert!(state(&eq).iter().any(|&z| z != 0.0));
        let mut block = vec![0.0f32; 1024 * 2];
        for _ in 0..(48000 * 10 / 1024) {
            block.fill(0.0);
            eq.process(&mut block, &p);
        }
        assert!(eq.cur_db.iter().all(|&g| g == 12.0));
        assert!(state(&eq).iter().all(|&z| z == 0.0), "{:?}", state(&eq));
    }

    #[test]
    fn gains() {
        assert_eq!(channel_gains(1.0, 0.0), (1.0, 1.0));
        assert_eq!(channel_gains(0.5, 0.0), (0.25, 0.25));
        assert_eq!(channel_gains(1.0, 1.0), (0.0, 1.0));
        assert_eq!(channel_gains(1.0, -0.5), (1.0, 0.5));
    }
}
