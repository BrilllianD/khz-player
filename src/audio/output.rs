//! cpal output stream. The realtime callback pulls stereo f32 from the ring,
//! applies EQ, taps the spectrum, applies volume/balance and writes the device buffer.
//! No locks, no allocation in the steady state, no logging.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, FromSample, I24, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize,
    SupportedStreamConfig, SupportedStreamConfigRange, U24,
};

use crate::audio::dsp::{Equalizer, approach, channel_gains, soft_clip};
use crate::audio::shared::Shared;

/// Ring capacity in seconds of stereo audio.
const RING_SECONDS: f32 = 0.3;
pub const TAP_CAPACITY: usize = 8192;
/// Pause/resume fade length.
const FADE_SECONDS: f32 = 0.006;
/// Time for the volume/balance gain to travel the full 0..1 range.
const GAIN_RAMP_SECONDS: f32 = 0.010;

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

    let supported = match device.supported_output_configs() {
        Ok(it) => it.collect(),
        Err(e) => {
            tracing::debug!("supported output configs: {e}");
            Vec::new()
        }
    };
    let attempts = attempt_list(&supported, &default);

    shared.out_rate.store(rate, Ordering::Relaxed);
    let ring_len = ((rate as f32 * RING_SECONDS) as usize) * 2;
    let mut last_err = None;
    for (format, config) in attempts {
        // Fresh ring, tap and callback state per attempt: a rejected config drops
        // them, so the returned ends always belong to the stream that started.
        let (ring_tx, ring_rx) = rtrb::RingBuffer::<f32>::new(ring_len);
        let (tap_tx, tap_rx) = rtrb::RingBuffer::<f32>::new(TAP_CAPACITY);
        let state = CallbackState::new(
            shared.clone(),
            ring_rx,
            tap_tx,
            rate,
            config.channels as usize,
        );
        let stream = match build(&device, format, config, state) {
            Ok(stream) => stream,
            Err(e) => {
                tracing::debug!("stream config {format} {config:?} failed: {e}");
                last_err = Some(e.to_string());
                continue;
            }
        };
        if let Err(e) = stream.play() {
            tracing::debug!("stream config {format} {config:?} failed to start: {e}");
            last_err = Some(e.to_string());
            continue;
        }
        tracing::info!(
            "audio output: {device_name}, {format}, {} ch, {rate} Hz, buffer {:?}",
            config.channels,
            config.buffer_size
        );
        return Ok(OutputParts {
            output: Output { _stream: stream },
            ring: ring_tx,
            tap: tap_rx,
        });
    }
    Err(anyhow!(
        "cannot open output stream: {}",
        last_err.unwrap_or_default()
    ))
}

/// Preferred device buffer, in frames.
const BUFFER_FRAMES: u32 = 1024;

/// Orders the configs to try: those that support the default rate, stereo
/// first, then more channels, then mono; within that F32 > I32 > I24 > I16 >
/// U16 > the rest. Each gets a fixed 1024-frame buffer (when the device allows
/// it) and then the default buffer. The device default config comes last, so
/// the scoring never does worse than opening the default.
fn attempt_list(
    supported: &[SupportedStreamConfigRange],
    default: &SupportedStreamConfig,
) -> Vec<(SampleFormat, StreamConfig)> {
    let rate = default.sample_rate();
    let mut ranked: Vec<_> = supported
        .iter()
        .filter(|c| c.channels() > 0 && c.contains_rate(rate))
        .filter_map(|c| Some((format_rank(c.sample_format())?, c)))
        .collect();
    ranked.sort_by_key(|&(fmt, c)| (channel_rank(c.channels()), c.channels(), fmt));

    let mut out = Vec::new();
    let mut push = |format: SampleFormat, channels: u16, fixed_ok: bool| {
        let format = playable(format);
        let sizes = [
            fixed_ok.then_some(BufferSize::Fixed(BUFFER_FRAMES)),
            Some(BufferSize::Default),
        ];
        for buffer_size in sizes.into_iter().flatten() {
            let attempt = (
                format,
                StreamConfig {
                    channels,
                    sample_rate: rate,
                    buffer_size,
                },
            );
            if !out.contains(&attempt) {
                out.push(attempt);
            }
        }
    };
    for (_, c) in ranked {
        let fixed_ok = match *c.buffer_size() {
            SupportedBufferSize::Range { min, max } => (min..=max).contains(&BUFFER_FRAMES),
            SupportedBufferSize::Unknown => true,
        };
        push(c.sample_format(), c.channels(), fixed_ok);
    }
    push(default.sample_format(), default.channels().max(1), true);
    out
}

/// Stereo, then surround (extra channels get silence), then mono.
fn channel_rank(channels: u16) -> u8 {
    match channels {
        2 => 0,
        1 => 2,
        _ => 1,
    }
}

/// Lower is better; `None` for formats the callback cannot write (DSD).
fn format_rank(format: SampleFormat) -> Option<u8> {
    Some(match format {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I24 => 2,
        SampleFormat::I16 => 3,
        SampleFormat::U16 => 4,
        SampleFormat::F64 => 5,
        SampleFormat::U32 => 6,
        SampleFormat::U24 => 7,
        SampleFormat::I64 => 8,
        SampleFormat::U64 => 9,
        SampleFormat::I8 => 10,
        SampleFormat::U8 => 11,
        _ => return None,
    })
}

/// The format `build` will actually open: anything it has no arm for runs as F32.
fn playable(format: SampleFormat) -> SampleFormat {
    if format_rank(format).is_some() {
        format
    } else {
        SampleFormat::F32
    }
}

fn build(
    device: &cpal::Device,
    format: SampleFormat,
    config: StreamConfig,
    state: CallbackState,
) -> Result<cpal::Stream, cpal::Error> {
    // `SampleFormat` is non-exhaustive; unknown formats fall back to f32.
    match format {
        SampleFormat::I8 => build_typed::<i8>(device, config, state),
        SampleFormat::I16 => build_typed::<i16>(device, config, state),
        SampleFormat::I24 => build_typed::<I24>(device, config, state),
        SampleFormat::I32 => build_typed::<i32>(device, config, state),
        SampleFormat::I64 => build_typed::<i64>(device, config, state),
        SampleFormat::U8 => build_typed::<u8>(device, config, state),
        SampleFormat::U16 => build_typed::<u16>(device, config, state),
        SampleFormat::U24 => build_typed::<U24>(device, config, state),
        SampleFormat::U32 => build_typed::<u32>(device, config, state),
        SampleFormat::U64 => build_typed::<u64>(device, config, state),
        SampleFormat::F64 => build_typed::<f64>(device, config, state),
        _ => build_typed::<f32>(device, config, state),
    }
}

fn build_typed<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut st: CallbackState,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let shared = st.shared.clone();
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| st.fill(data),
        // Xruns can come in bursts; count them and let the engine log the total.
        move |e| {
            if e.kind() == cpal::ErrorKind::Xrun {
                shared.xruns.fetch_add(1, Ordering::Relaxed);
            } else {
                tracing::warn!("audio stream error: {e}");
            }
        },
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
    /// Current left/right gain, ramped per frame toward `channel_gains(...)`.
    gain: (f32, f32),
    gain_step: f32,
}

impl CallbackState {
    fn new(
        shared: Arc<Shared>,
        ring: rtrb::Consumer<f32>,
        tap: rtrb::Producer<f32>,
        rate: u32,
        channels: usize,
    ) -> Self {
        let gain = channel_gains(shared.volume.load(), shared.balance.load());
        Self {
            shared,
            ring,
            tap,
            eq: Equalizer::new(rate as f64),
            channels: channels.max(1),
            work: vec![0.0; 8192 * 2],
            fade: 0.0,
            fade_step: 1.0 / (rate as f32 * FADE_SECONDS).max(1.0),
            gain,
            gain_step: 1.0 / (rate as f32 * GAIN_RAMP_SECONDS).max(1.0),
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
            self.fade = approach(self.fade, target, self.fade_step);
            self.gain.0 = approach(self.gain.0, gl, self.gain_step);
            self.gain.1 = approach(self.gain.1, gr, self.gain_step);
            let l = soft_clip(work[i * 2] * self.gain.0 * self.fade);
            let r = soft_clip(work[i * 2 + 1] * self.gain.1 * self.fade);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn range(
        channels: u16,
        min: u32,
        max: u32,
        format: SampleFormat,
    ) -> SupportedStreamConfigRange {
        SupportedStreamConfigRange::new(
            channels,
            min,
            max,
            SupportedBufferSize::Range { min: 64, max: 8192 },
            format,
        )
    }

    #[test]
    fn attempt_list_scores_channels_then_format_and_ends_with_default() {
        let supported = [
            range(1, 8000, 192000, SampleFormat::F32),
            range(2, 8000, 192000, SampleFormat::I16),
            range(6, 8000, 192000, SampleFormat::F32),
            range(2, 8000, 192000, SampleFormat::I24),
            range(2, 8000, 44100, SampleFormat::F32), // no 48 kHz
            range(2, 8000, 192000, SampleFormat::DsdU8),
        ];
        let default = range(2, 8000, 192000, SampleFormat::I16).with_sample_rate(48000);
        let got: Vec<_> = attempt_list(&supported, &default)
            .into_iter()
            .map(|(f, c)| (c.channels, f, c.buffer_size))
            .collect();
        let fixed = BufferSize::Fixed(BUFFER_FRAMES);
        let def = BufferSize::Default;
        assert_eq!(
            got,
            [
                (2, SampleFormat::I24, fixed),
                (2, SampleFormat::I24, def),
                (2, SampleFormat::I16, fixed),
                (2, SampleFormat::I16, def),
                (6, SampleFormat::F32, fixed),
                (6, SampleFormat::F32, def),
                (1, SampleFormat::F32, fixed),
                (1, SampleFormat::F32, def),
            ]
        );
    }

    #[test]
    fn attempt_list_falls_back_to_default_without_ranges() {
        let default = range(2, 8000, 192000, SampleFormat::DsdU16).with_sample_rate(44100);
        let got = attempt_list(&[], &default);
        assert_eq!(got.len(), 2);
        assert!(
            got.iter()
                .all(|(f, c)| *f == SampleFormat::F32 && c.channels == 2 && c.sample_rate == 44100)
        );
        assert_eq!(got[1].1.buffer_size, BufferSize::Default);
    }
}
