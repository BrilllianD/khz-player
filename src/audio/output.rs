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
    /// Output sample rate of the stream.
    pub rate: u32,
}

/// Opens the default output device. Does not touch `shared.out_rate`: the
/// caller hands `rate` to the engine together with `ring`, so position and
/// rate change at the same moment. `repaint` wakes the UI when the stream
/// fails for good.
pub fn open(shared: Arc<Shared>, repaint: egui::Context) -> anyhow::Result<OutputParts> {
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
        let stream = match build(&device, format, config, state, repaint.clone()) {
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
            rate,
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
    repaint: egui::Context,
) -> Result<cpal::Stream, cpal::Error> {
    // `SampleFormat` is non-exhaustive; unknown formats fall back to f32.
    match format {
        SampleFormat::I8 => build_typed::<i8>(device, config, state, repaint),
        SampleFormat::I16 => build_typed::<i16>(device, config, state, repaint),
        SampleFormat::I24 => build_typed::<I24>(device, config, state, repaint),
        SampleFormat::I32 => build_typed::<i32>(device, config, state, repaint),
        SampleFormat::I64 => build_typed::<i64>(device, config, state, repaint),
        SampleFormat::U8 => build_typed::<u8>(device, config, state, repaint),
        SampleFormat::U16 => build_typed::<u16>(device, config, state, repaint),
        SampleFormat::U24 => build_typed::<U24>(device, config, state, repaint),
        SampleFormat::U32 => build_typed::<u32>(device, config, state, repaint),
        SampleFormat::U64 => build_typed::<u64>(device, config, state, repaint),
        SampleFormat::F64 => build_typed::<f64>(device, config, state, repaint),
        _ => build_typed::<f32>(device, config, state, repaint),
    }
}

fn build_typed<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut st: CallbackState,
    repaint: egui::Context,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let shared = st.shared.clone();
    device.build_output_stream::<T, _, _>(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| st.fill(data),
        move |e| match e.kind() {
            // Xruns can come in bursts; count them and let the engine log the total.
            cpal::ErrorKind::Xrun => {
                shared.xruns.fetch_add(1, Ordering::Relaxed);
            }
            // The stream keeps running: no realtime priority, or rerouted
            // to a new default device.
            cpal::ErrorKind::RealtimeDenied | cpal::ErrorKind::DeviceChanged => {
                tracing::info!("audio stream: {e}");
            }
            // Anything else may have stopped the stream (device unplugged,
            // sound server restarted); the UI reopens the output.
            _ => {
                if shared.device_lost.swap(true, Ordering::AcqRel) {
                    tracing::debug!("audio stream error: {e}");
                } else {
                    tracing::warn!("audio stream error: {e}");
                    repaint.request_repaint();
                }
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
    use cpal::Sample;

    use crate::audio::dsp::EqParams;
    use crate::config::BANDS;

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

    const RATE: u32 = 48000;

    /// A stereo callback at 48 kHz on plain ring ends. Returns the engine side
    /// of the ring, the UI side of the tap and the state under test.
    fn callback(
        shared: &Arc<Shared>,
        ring_len: usize,
        tap_len: usize,
    ) -> (rtrb::Producer<f32>, rtrb::Consumer<f32>, CallbackState) {
        let (ring_tx, ring_rx) = rtrb::RingBuffer::<f32>::new(ring_len);
        let (tap_tx, tap_rx) = rtrb::RingBuffer::<f32>::new(tap_len);
        let st = CallbackState::new(shared.clone(), ring_rx, tap_tx, RATE, 2);
        (ring_tx, tap_rx, st)
    }

    fn push(ring: &mut rtrb::Producer<f32>, frames: &[(f32, f32)]) {
        for &(l, r) in frames {
            ring.push(l).unwrap();
            ring.push(r).unwrap();
        }
    }

    /// Runs the callback over an empty ring long enough for the 6 ms fade-in to finish.
    fn warm_up(st: &mut CallbackState) {
        st.fill(&mut [0.0f32; 1024]);
        assert_eq!(st.fade, 1.0);
    }

    #[test]
    fn fill_flush_ack_clears_ring_and_flag() {
        let shared = Arc::new(Shared::new(1.0, 0.0));
        let (mut ring, _tap, mut st) = callback(&shared, 1024, 64);
        push(&mut ring, &[(0.5, 0.5); 100]);
        shared.flush.store(true, Ordering::Release);

        let mut out = [1.0f32; 64];
        st.fill(&mut out);

        assert!(!shared.flush.load(Ordering::Acquire));
        assert_eq!(ring.slots(), 1024, "ring still holds samples");
        assert_eq!(shared.consumed.load(Ordering::Acquire), 0);
        assert!(out.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn fill_paused_at_fade_zero_outputs_equilibrium_and_consumes_nothing() {
        let shared = Arc::new(Shared::new(1.0, 0.0));
        let (mut ring, _tap, mut st) = callback(&shared, 1024, 64);
        push(&mut ring, &[(0.5, 0.5); 64]);
        shared.paused.store(true, Ordering::Relaxed);

        // u16 silence is the midpoint, not 0.
        let mut out = [0u16; 64];
        st.fill(&mut out);

        assert!(out.iter().all(|&s| s == u16::EQUILIBRIUM));
        assert_eq!(ring.slots(), 1024 - 128);
        assert_eq!(shared.consumed.load(Ordering::Acquire), 0);
    }

    #[test]
    fn fill_underrun_zero_fills_and_counts_only_popped_frames() {
        let shared = Arc::new(Shared::new(1.0, 0.0));
        let (mut ring, _tap, mut st) = callback(&shared, 1024, 64);
        push(&mut ring, &[(0.5, -0.5); 10]);

        let mut out = [1.0f32; 64 * 2];
        st.fill(&mut out);

        assert_eq!(shared.consumed.load(Ordering::Acquire), 10);
        assert_eq!(ring.slots(), 1024);
        assert!(out[..20].iter().all(|&s| s != 0.0));
        assert!(out[20..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn fill_taps_mono_post_eq_and_counts_tap_overflow() {
        let shared = Arc::new(Shared::new(0.5, 0.0));
        let params = EqParams {
            enabled: true,
            preamp_db: -6.0,
            bands_db: [0.0; BANDS],
        };
        shared.set_eq(&params);
        let (mut ring, mut tap, mut st) = callback(&shared, 1024, 16);
        let input: Vec<(f32, f32)> = (0..64)
            .map(|i| {
                let x = (i as f32 * 0.3).sin();
                (0.6 * x, 0.2 * x)
            })
            .collect();
        push(&mut ring, &input);

        st.fill(&mut [0.0f32; 64 * 2]);

        // Same EQ run separately on the same input: the tap must match it exactly.
        let mut expect: Vec<f32> = input.iter().flat_map(|&(l, r)| [l, r]).collect();
        Equalizer::new(RATE as f64).process(&mut expect, &params);
        let got: Vec<f32> = std::iter::from_fn(|| tap.pop().ok()).collect();
        assert_eq!(got.len(), 16);
        for (i, &s) in got.iter().enumerate() {
            let want = (expect[i * 2] + expect[i * 2 + 1]) * 0.5;
            let raw = (input[i].0 + input[i].1) * 0.5;
            assert!((s - want).abs() < 1e-6, "frame {i}: {s} vs {want}");
            if raw.abs() > 0.05 {
                assert!((s - raw).abs() > 1e-3, "frame {i} is not post-EQ");
            }
        }
        assert_eq!(shared.tap_overflow.load(Ordering::Relaxed), 64 - 16);
    }

    #[test]
    fn fill_i16_maps_full_scale_through_soft_clip() {
        let shared = Arc::new(Shared::new(1.0, 0.0));
        let (mut ring, _tap, mut st) = callback(&shared, 1024, 64);
        warm_up(&mut st);
        push(&mut ring, &[(1.0, -1.0); 16]);

        let mut out = [0i16; 16 * 2];
        st.fill(&mut out);

        let want = soft_clip(1.0) * i16::MAX as f32;
        for &[l, r] in out.as_chunks::<2>().0 {
            assert!((l as f32 - want).abs() <= 2.0, "{} vs {want}", l);
            assert!((r as f32 + want).abs() <= 2.0, "{} vs -{want}", r);
        }
    }

    #[test]
    fn fill_ramps_volume_step_over_ten_ms() {
        let shared = Arc::new(Shared::new(0.0, 0.0));
        let (mut ring, _tap, mut st) = callback(&shared, 4096, 64);
        warm_up(&mut st);
        assert_eq!(st.gain, (0.0, 0.0));
        shared.volume.store(1.0);
        push(&mut ring, &[(0.5, 0.5); 600]);

        let mut out = [0.0f32; 600 * 2];
        st.fill(&mut out);

        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        assert!(left[0] < 0.002, "first frame {}", left[0]);
        assert!((left[240] - 0.25).abs() < 0.005, "mid-ramp {}", left[240]);
        assert!((left[480] - 0.5).abs() < 1e-6, "frame 480 {}", left[480]);
        assert!(left.windows(2).all(|w| w[1] >= w[0]));
    }
}
