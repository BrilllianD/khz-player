//! Lock-free state shared between UI, engine thread and the realtime callback.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};

use crate::audio::dsp::EqParams;
use crate::config::BANDS;

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
    pub eq_enabled: AtomicBool,
    pub eq_preamp: AtomicF32,
    pub eq_bands: [AtomicF32; BANDS],
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
            eq_enabled: AtomicBool::new(false),
            eq_preamp: AtomicF32::new(0.0),
            eq_bands: Default::default(),
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

    /// Frames of the current track that have been played.
    pub fn position_frames(&self) -> u64 {
        let c = self.consumed.load(Ordering::Acquire) as i64;
        (c - self.track_start.load(Ordering::Acquire)).max(0) as u64
    }

    pub fn out_rate(&self) -> u32 {
        self.out_rate.load(Ordering::Relaxed)
    }
}
