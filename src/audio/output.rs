//! cpal output stream. The realtime callback pulls stereo f32 from the ring,
//! applies EQ, taps the spectrum, applies volume/balance and writes the device buffer.
//! No locks, no allocation in the steady state, no logging.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, SampleFormat, SizedSample, StreamConfig};

use crate::audio::dsp::{Equalizer, channel_gains};
use crate::audio::shared::Shared;

/// Ring capacity in seconds of stereo audio.
const RING_SECONDS: f32 = 0.3;
pub const TAP_CAPACITY: usize = 8192;
/// Pause/resume fade length.
const FADE_SECONDS: f32 = 0.006;

pub struct Output {
    _stream: cpal::Stream,
}

pub struct OutputParts {
    pub output: Output,
    /// Engine side of the audio ring (interleaved stereo f32 at `rate`).
    pub ring: rtrb::Producer<f32>,
    /// UI side of the spectrum tap (mono f32 at `rate`).
    pub tap: rtrb::Consumer<f32>,
}

pub fn open(shared: Arc<Shared>) -> anyhow::Result<OutputParts> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("no default output device"))?;
    let device_name = device.to_string();
    let default = device
        .default_output_config()
        .context("default output config")?;
    let rate = default.sample_rate();

    // Prefer f32 at the default rate; otherwise fall back to the default format.
    let format = if default.sample_format() == SampleFormat::F32 {
        SampleFormat::F32
    } else {
        device
            .supported_output_configs()
            .ok()
            .and_then(|mut it| {
                it.find(|c| c.sample_format() == SampleFormat::F32 && c.contains_rate(rate))
            })
            .map(|_| SampleFormat::F32)
            .unwrap_or(default.sample_format())
    };

    shared.out_rate.store(rate, Ordering::Relaxed);
    let ring_len = ((rate as f32 * RING_SECONDS) as usize) * 2;
    let (ring_tx, ring_rx) = rtrb::RingBuffer::<f32>::new(ring_len);
    let (tap_tx, tap_rx) = rtrb::RingBuffer::<f32>::new(TAP_CAPACITY);

    let mut attempts = Vec::new();
    for channels in [2u16, default.channels()] {
        for buffer_size in [BufferSize::Fixed(1024), BufferSize::Default] {
            attempts.push(StreamConfig {
                channels,
                sample_rate: rate,
                buffer_size,
            });
        }
    }
    attempts.dedup();

    let mut state = Some(CallbackState::new(shared, ring_rx, tap_tx, rate, 2));
    let mut last_err = None;
    for config in attempts {
        let result = match format {
            SampleFormat::I16 => build::<i16>(&device, config, &mut state),
            SampleFormat::I32 => build::<i32>(&device, config, &mut state),
            SampleFormat::U16 => build::<u16>(&device, config, &mut state),
            _ => build::<f32>(&device, config, &mut state),
        };
        match result {
            Ok(stream) => {
                stream.play().context("start stream")?;
                tracing::info!(
                    "audio output: {device_name}, {rate} Hz, {} ch, {format:?}, {:?}",
                    config.channels,
                    config.buffer_size
                );
                return Ok(OutputParts {
                    output: Output { _stream: stream },
                    ring: ring_tx,
                    tap: tap_rx,
                });
            }
            Err(e) => {
                tracing::debug!("stream config {config:?} failed: {e}");
                last_err = Some(e);
                if state.is_none() {
                    break;
                }
            }
        }
    }
    Err(anyhow!(
        "cannot open output stream: {}",
        last_err.map(|e| e.to_string()).unwrap_or_default()
    ))
}

/// Probes `config` with a no-op callback first so `state` survives a rejected
/// config and can be reused for the next attempt.
fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    state: &mut Option<CallbackState>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    drop(device.build_output_stream::<T, _, _>(
        config,
        |_: &mut [T], _: &cpal::OutputCallbackInfo| {},
        |_| {},
        None,
    )?);
    let mut st = state.take().expect("callback state");
    st.channels = config.channels.max(1) as usize;
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| st.fill(data),
        |e| tracing::warn!("audio stream error: {e}"),
        None,
    )
}

struct CallbackState {
    shared: Arc<Shared>,
    ring: rtrb::Consumer<f32>,
    tap: rtrb::Producer<f32>,
    eq: Equalizer,
    channels: usize,
    work: Vec<f32>,
    fade: f32,
    fade_step: f32,
}

impl CallbackState {
    fn new(
        shared: Arc<Shared>,
        ring: rtrb::Consumer<f32>,
        tap: rtrb::Producer<f32>,
        rate: u32,
        channels: usize,
    ) -> Self {
        Self {
            shared,
            ring,
            tap,
            eq: Equalizer::new(rate as f64),
            channels: channels.max(1),
            work: vec![0.0; 8192 * 2],
            fade: 0.0,
            fade_step: 1.0 / (rate as f32 * FADE_SECONDS).max(1.0),
        }
    }

    fn fill<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T]) {
        let shared = &*self.shared;
        let ch = self.channels;
        let frames = data.len() / ch;

        if shared.flush.load(Ordering::Acquire) {
            let n = self.ring.slots();
            if let Ok(chunk) = self.ring.read_chunk(n) {
                chunk.commit_all();
            }
            self.fade = 0.0;
            shared.flush.store(false, Ordering::Release);
        }

        let paused = shared.paused.load(Ordering::Relaxed);
        if paused && self.fade <= 0.0 {
            data.fill(T::EQUILIBRIUM);
            return;
        }

        if self.work.len() < frames * 2 {
            // Only if the device suddenly asks for a huge buffer.
            self.work.resize(frames * 2, 0.0);
        }
        let work = &mut self.work[..frames * 2];
        let avail = (self.ring.slots() / 2).min(frames);
        let (got, _) = self.ring.pop_partial_slice(&mut work[..avail * 2]);
        let got = got.len() / 2;
        work[got * 2..].fill(0.0);
        if got > 0 {
            shared.consumed.fetch_add(got as u64, Ordering::AcqRel);
        }

        let params = shared.eq_params();
        self.eq.process(&mut work[..got * 2], &params);

        // Spectrum tap: mono, post-EQ, pre-volume.
        let room = self.tap.slots().min(got);
        if let Ok(mut chunk) = self.tap.write_chunk(room) {
            let (a, b) = chunk.as_mut_slices();
            for (i, s) in a.iter_mut().chain(b.iter_mut()).enumerate() {
                *s = (work[i * 2] + work[i * 2 + 1]) * 0.5;
            }
            chunk.commit_all();
        }
        if room < got {
            shared
                .tap_overflow
                .fetch_add((got - room) as u64, Ordering::Relaxed);
        }

        let (gl, gr) = channel_gains(shared.volume.load(), shared.balance.load());
        let target = if paused { 0.0 } else { 1.0 };
        for (i, out) in data.chunks_exact_mut(ch).enumerate() {
            if self.fade != target {
                self.fade = if target > self.fade {
                    (self.fade + self.fade_step).min(1.0)
                } else {
                    (self.fade - self.fade_step).max(0.0)
                };
            }
            let l = (work[i * 2] * gl * self.fade).clamp(-1.0, 1.0);
            let r = (work[i * 2 + 1] * gr * self.fade).clamp(-1.0, 1.0);
            match ch {
                1 => out[0] = T::from_sample((l + r) * 0.5),
                _ => {
                    out[0] = T::from_sample(l);
                    out[1] = T::from_sample(r);
                    for o in &mut out[2..] {
                        *o = T::EQUILIBRIUM;
                    }
                }
            }
        }
    }
}
