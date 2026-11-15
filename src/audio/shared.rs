//! Lock-free state shared between UI, engine thread and the realtime callback.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU32, AtomicU64, Ordering};

use crate::audio::dsp::EqParams;
use crate::config::BANDS;
use crate::library::ReplayGain;

/// ReplayGain mode codes stored in `Shared::rg_mode`. Any other value acts like `RG_TRACK`.
pub const RG_OFF: u8 = 0;
pub const RG_TRACK: u8 = 1;
pub const RG_ALBUM: u8 = 2;

#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    pub fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn store(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
}

pub struct Shared {
    /// Output sample rate, set once the stream is open.
    pub out_rate: AtomicU32,
    /// Frames taken from the ring by the callback since startup (monotonic).
    pub consumed: AtomicU64,
    /// Value of `consumed` that corresponds to position 0 of the current track.
    /// Negative after seeking forward early in the session.
    pub track_start: AtomicI64,
    /// Engine asks the callback to discard everything buffered; callback clears it.
    pub flush: AtomicBool,
    /// When true the callback fades out and outputs silence without consuming.
    pub paused: AtomicBool,
    pub volume: AtomicF32,
    pub balance: AtomicF32,
    /// Sleep-timer fade, multiplied into the volume target; 1.0 when no fade runs.
    pub sleep_gain: AtomicF32,
    pub eq_enabled: AtomicBool,
    pub eq_preamp: AtomicF32,
    pub eq_bands: [AtomicF32; BANDS],
    /// ReplayGain mode (`RG_*`), preamp and gain of untagged tracks; the engine
    /// reads them on every `fill`, so a change is audible after the buffer drains.
    pub rg_mode: AtomicU8,
    pub rg_preamp_db: AtomicF32,
    pub rg_untagged_db: AtomicF32,
    /// Number of frames in the spectrum tap that were dropped (diagnostics only).
    pub tap_overflow: AtomicU64,
    /// Buffer underruns/overruns reported by the stream; the engine logs the count.
    pub xruns: AtomicU64,
    /// The stream reported a fatal error (device gone, server restarted) or
    /// never opened. The engine stops waiting for flush acks; the UI reopens.
    pub device_lost: AtomicBool,
}

impl Shared {
    pub fn new(volume: f32, balance: f32) -> Self {
        Self {
            out_rate: AtomicU32::new(48000),
            consumed: AtomicU64::new(0),
            track_start: AtomicI64::new(0),
            flush: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            volume: AtomicF32::new(volume),
            balance: AtomicF32::new(balance),
            sleep_gain: AtomicF32::new(1.0),
            eq_enabled: AtomicBool::new(false),
            eq_preamp: AtomicF32::new(0.0),
            eq_bands: Default::default(),
            rg_mode: AtomicU8::new(RG_OFF),
            rg_preamp_db: AtomicF32::new(0.0),
            rg_untagged_db: AtomicF32::new(0.0),
            tap_overflow: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            device_lost: AtomicBool::new(false),
        }
    }

    pub fn eq_params(&self) -> EqParams {
        let mut bands = [0.0; BANDS];
        for (b, a) in bands.iter_mut().zip(&self.eq_bands) {
            *b = a.load();
        }
        EqParams {
            enabled: self.eq_enabled.load(Ordering::Relaxed),
            preamp_db: self.eq_preamp.load(),
            bands_db: bands,
        }
    }

    pub fn set_eq(&self, p: &EqParams) {
        self.eq_enabled.store(p.enabled, Ordering::Relaxed);
        self.eq_preamp.store(p.preamp_db);
        for (a, &b) in self.eq_bands.iter().zip(&p.bands_db) {
            a.store(b);
        }
    }

    /// Linear ReplayGain factor for a track with tags `rg`.
    pub fn rg_factor(&self, rg: &ReplayGain) -> f32 {
        replay_gain_factor(
            rg,
            self.rg_mode.load(Ordering::Relaxed),
            self.rg_preamp_db.load(),
            self.rg_untagged_db.load(),
        )
    }

    /// Frames of the current track that have been played.
    pub fn position_frames(&self) -> u64 {
        let c = self.consumed.load(Ordering::Acquire) as i64;
        (c - self.track_start.load(Ordering::Acquire)).max(0) as u64
    }

    pub fn out_rate(&self) -> u32 {
        self.out_rate.load(Ordering::Relaxed)
    }
}

/// Linear factor for a track: the tagged gain for `mode` with fallbacks, else
/// `untagged_db`, plus `preamp_db`, capped so the peak stays at or below 1.0.
pub fn replay_gain_factor(rg: &ReplayGain, mode: u8, preamp_db: f32, untagged_db: f32) -> f32 {
    if mode == RG_OFF {
        return 1.0;
    }
    let pair = |g: Option<f32>, peak: Option<f32>| g.filter(|g| g.is_finite()).map(|g| (g, peak));
    let track = pair(rg.track_gain_db, rg.track_peak);
    let album = pair(rg.album_gain_db, rg.album_peak);
    let chosen = if mode == RG_ALBUM { album.or(track) } else { track.or(album) };
    let (db, peak) = chosen.unwrap_or((untagged_db, None));
    let mut lin = 10f32.powf((db + preamp_db) / 20.0);
    if let Some(p) = peak.filter(|p| p.is_finite() && *p > 0.0) {
        lin = lin.min(1.0 / p);
    }
    lin
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(track: Option<f32>, album: Option<f32>) -> ReplayGain {
        ReplayGain {
            track_gain_db: track,
            album_gain_db: album,
            ..Default::default()
        }
    }

    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-3, "{a} != {b}");
    }

    #[test]
    fn off_is_exactly_one() {
        assert_eq!(replay_gain_factor(&tagged(Some(-6.0), None), RG_OFF, 3.0, -6.0), 1.0);
        assert_eq!(replay_gain_factor(&ReplayGain::default(), RG_OFF, 0.0, -6.0), 1.0);
    }

    #[test]
    fn track_gain_applies() {
        near(replay_gain_factor(&tagged(Some(-6.0206), None), RG_TRACK, 0.0, 0.0), 0.5);
    }

    #[test]
    fn track_mode_falls_back_to_album() {
        near(replay_gain_factor(&tagged(None, Some(-6.0206)), RG_TRACK, 0.0, 0.0), 0.5);
    }

    #[test]
    fn album_mode_prefers_album() {
        near(replay_gain_factor(&tagged(Some(0.0), Some(-6.0206)), RG_ALBUM, 0.0, 0.0), 0.5);
        near(replay_gain_factor(&tagged(Some(-6.0206), Some(0.0)), RG_TRACK, 0.0, 0.0), 0.5);
    }

    #[test]
    fn untagged_uses_untagged_gain() {
        near(replay_gain_factor(&ReplayGain::default(), RG_TRACK, 0.0, -6.0206), 0.5);
    }

    #[test]
    fn preamp_is_added() {
        near(replay_gain_factor(&tagged(Some(0.0), None), RG_TRACK, 6.0206, 0.0), 2.0);
    }

    #[test]
    fn peak_caps_the_gain() {
        let rg = ReplayGain {
            track_gain_db: Some(6.0),
            track_peak: Some(0.9),
            ..Default::default()
        };
        near(replay_gain_factor(&rg, RG_TRACK, 0.0, 0.0), 1.0 / 0.9);
    }

    #[test]
    fn bad_peak_is_ignored() {
        for peak in [Some(0.0), None, Some(f32::NAN)] {
            let rg = ReplayGain {
                track_gain_db: Some(6.0206),
                track_peak: peak,
                ..Default::default()
            };
            near(replay_gain_factor(&rg, RG_TRACK, 0.0, 0.0), 2.0);
        }
    }

    #[test]
    fn nan_gain_falls_back_to_other() {
        near(replay_gain_factor(&tagged(Some(f32::NAN), Some(-6.0206)), RG_TRACK, 0.0, 0.0), 0.5);
    }
}
