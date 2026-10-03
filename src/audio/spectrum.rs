//! UI-side spectrum analyzer fed by the callback tap.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

pub const BARS: usize = 20;
const N: usize = 2048;
const F_MIN: f32 = 50.0;
const F_MAX: f32 = 16000.0;
const DB_FLOOR: f32 = -60.0;
/// Music falls off ~3-5 dB per octave, which pins the bass bars and leaves the
/// treble low. Tilt by this much per octave around 1 kHz (pink noise reads flat).
const TILT_DB_PER_OCT: f32 = 3.0;
/// Fall speeds in units per second (bars are 0..1).
const BAR_FALL: f32 = 2.4;
const PEAK_FALL: f32 = 0.6;
const PEAK_HOLD: f32 = 0.5;

pub struct Spectrum {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    history: Vec<f32>,
    write: usize,
    input: Vec<f32>,
    output: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    edges: [usize; BARS + 1],
    rate: u32,
    pub bars: [f32; BARS],
    pub peaks: [f32; BARS],
    hold: [f32; BARS],
    fresh: bool,
}

/// Level correction for bar `k`, taken at the band's geometric center.
fn tilt_db(k: usize) -> f32 {
    let center = F_MIN * (F_MAX / F_MIN).powf((k as f32 + 0.5) / BARS as f32);
    TILT_DB_PER_OCT * (center / 1000.0).log2()
}

impl Spectrum {
    pub fn new(rate: u32) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N);
        let window = (0..N)
            .map(|i| {
                let x = std::f32::consts::PI * 2.0 * i as f32 / (N - 1) as f32;
                0.5 - 0.5 * x.cos()
            })
            .collect();
        let output = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        let mut s = Self {
            fft,
            window,
            history: vec![0.0; N],
            write: 0,
            input: vec![0.0; N],
            output,
            scratch,
            edges: [0; BARS + 1],
            rate,
            bars: [0.0; BARS],
            peaks: [0.0; BARS],
            hold: [0.0; BARS],
            fresh: false,
        };
        s.compute_edges();
        s
    }

    fn compute_edges(&mut self) {
        let max_bin = N / 2;
        let ratio = F_MAX / F_MIN;
        for k in 0..=BARS {
            let f = F_MIN * ratio.powf(k as f32 / BARS as f32);
            let bin = (f * N as f32 / self.rate.max(1) as f32).round() as usize;
            self.edges[k] = bin.clamp(1, max_bin);
        }
    }

    /// Pulls new samples from the tap.
    pub fn feed(&mut self, tap: &mut rtrb::Consumer<f32>) {
        let n = tap.slots();
        if n == 0 {
            return;
        }
        if let Ok(chunk) = tap.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            for &s in a.iter().chain(b) {
                self.history[self.write] = s;
                self.write = (self.write + 1) % N;
            }
            chunk.commit_all();
            self.fresh = true;
        }
    }

    /// Advances the animation by `dt` seconds. `active` is false when nothing plays,
    /// in which case bars just decay.
    pub fn update(&mut self, dt: f32, active: bool) {
        let mut target = [0.0f32; BARS];
        if active && self.fresh {
            self.fresh = false;
            for i in 0..N {
                self.input[i] = self.history[(self.write + i) % N] * self.window[i];
            }
            if self
                .fft
                .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
                .is_ok()
            {
                // A full-scale sine through a Hann window peaks at N/4.
                let norm = 4.0 / N as f32;
                for (k, t) in target.iter_mut().enumerate() {
                    let lo = self.edges[k];
                    let hi = self.edges[k + 1].max(lo + 1);
                    let mag = self.output[lo..hi.min(self.output.len())]
                        .iter()
                        .map(|c| c.norm())
                        .fold(0.0f32, f32::max)
                        * norm;
                    let db = 20.0 * mag.max(1e-9).log10() + tilt_db(k);
                    *t = ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0);
                }
            }
        } else if active {
            // No new samples this frame: keep the bars where they are.
            target = self.bars;
        }
        for (k, &t) in target.iter().enumerate() {
            if t >= self.bars[k] {
                self.bars[k] = t;
            } else {
                self.bars[k] = (self.bars[k] - BAR_FALL * dt).max(t).max(0.0);
            }
            if self.bars[k] >= self.peaks[k] {
                self.peaks[k] = self.bars[k];
                self.hold[k] = PEAK_HOLD;
            } else if self.hold[k] > 0.0 {
                self.hold[k] -= dt;
            } else {
                self.peaks[k] = (self.peaks[k] - PEAK_FALL * dt).max(self.bars[k]);
            }
        }
    }

    pub fn is_idle(&self) -> bool {
        self.bars.iter().chain(&self.peaks).all(|&v| v <= 0.001)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_sine(freq: f32, amp: f32) -> Spectrum {
        let rate = 48000;
        let mut s = Spectrum::new(rate);
        let (mut tx, mut rx) = rtrb::RingBuffer::new(4096);
        for i in 0..N {
            let v = (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * amp;
            tx.push(v).unwrap();
        }
        s.feed(&mut rx);
        s.update(0.033, true);
        s
    }

    #[test]
    fn sine_lights_the_right_band() {
        let s = with_sine(1000.0, 0.5);
        let loudest = (0..BARS)
            .max_by(|&a, &b| s.bars[a].total_cmp(&s.bars[b]))
            .unwrap();
        // 1 kHz on a 50..16000 log scale with 20 bars sits in bar ~10.
        let expected = ((1000.0f32 / F_MIN).ln() / (F_MAX / F_MIN).ln() * BARS as f32) as usize;
        assert!(loudest.abs_diff(expected) <= 1, "{loudest} vs {expected}");
        assert!(s.bars[loudest] > 0.8);
    }

    #[test]
    fn loud_bass_does_not_pin() {
        // -6 dBFS at 60 Hz is ordinary for a modern master's kick.
        let s = with_sine(60.0, 0.5);
        let top = s.bars.iter().copied().fold(0.0f32, f32::max);
        assert!(top > 0.5 && top < 0.8, "{top}");
    }

    #[test]
    fn decays_when_inactive() {
        let mut s = Spectrum::new(48000);
        s.bars = [1.0; BARS];
        s.peaks = [1.0; BARS];
        for _ in 0..100 {
            s.update(0.033, false);
        }
        assert!(s.is_idle());
    }
}
