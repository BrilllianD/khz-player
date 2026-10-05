//! symphonia 0.6 wrapper producing interleaved stereo f32.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, anyhow};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase, Timestamp};

#[derive(Debug, Clone, PartialEq)]
pub struct TrackInfo {
    pub path: PathBuf,
    pub sample_rate: u32,
    pub channels: usize,
    pub duration: Option<Duration>,
    /// Average bitrate in kbps estimated from file size and duration.
    pub kbps: Option<u32>,
    pub codec: String,
}

pub struct Decoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    pub info: TrackInfo,
    tmp: Vec<f32>,
    /// After an accurate seek, frames before this timestamp are dropped.
    skip_until: Option<Timestamp>,
}

impl Decoder {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let reader = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|e| anyhow!("unsupported format: {e}"))?;
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or_else(|| anyhow!("no audio track"))?;
        let track_id = track.id;
        let time_base = track.time_base;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| anyhow!("no audio codec parameters"))?
            .clone();
        let duration = time_base
            .zip(track.duration)
            .and_then(|(tb, d)| tb.calc_duration(d))
            .map(|t| t.as_secs_f64())
            .or_else(|| {
                let n = track.num_frames?;
                let r = params.sample_rate?;
                Some(n as f64 / r as f64)
            })
            .filter(|s| s.is_finite() && *s > 0.0)
            .map(Duration::from_secs_f64);
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| anyhow!("unsupported codec: {e}"))?;
        let kbps = duration
            .filter(|d| d.as_secs_f64() > 0.5)
            .map(|d| (size as f64 * 8.0 / d.as_secs_f64() / 1000.0).round() as u32);
        let codec = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_uppercase();
        let info = TrackInfo {
            path: path.to_path_buf(),
            sample_rate: params.sample_rate.unwrap_or(44100),
            channels: params.channels.as_ref().map_or(2, |c| c.count()),
            duration,
            kbps,
            codec,
        };
        Ok(Self {
            reader,
            decoder,
            track_id,
            time_base,
            info,
            tmp: Vec::new(),
            skip_until: None,
        })
    }

    /// Decodes the next packet and appends interleaved stereo frames to `out`.
    /// Returns `Ok(false)` at end of stream.
    pub fn next_frames(&mut self, out: &mut Vec<f32>) -> anyhow::Result<bool> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(false),
                Err(Error::ResetRequired) => return Ok(false),
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(anyhow!("read error: {e}")),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buf = match self.decoder.decode(&packet) {
                Ok(b) => b,
                Err(Error::DecodeError(e)) => {
                    tracing::debug!("decode error (skipped): {e}");
                    continue;
                }
                Err(Error::IoError(_)) => continue,
                Err(e) => return Err(anyhow!("decode error: {e}")),
            };
            let frames = buf.frames();
            if frames == 0 {
                continue;
            }
            let ch = buf.spec().channels().count().max(1);
            let rate = buf.spec().rate();
            if rate != self.info.sample_rate {
                // Rare (e.g. chained streams); the engine reacts to the change.
                self.info.sample_rate = rate;
            }
            buf.copy_to_vec_interleaved(&mut self.tmp);

            let mut skip = 0usize;
            if let Some(until) = self.skip_until {
                let behind = ticks_to_frames(until.get() - packet.pts.get(), self.time_base, rate);
                if behind >= frames as i64 {
                    continue;
                }
                skip = behind.max(0) as usize;
                self.skip_until = None;
            }

            mix_to_stereo(&self.tmp[skip * ch..frames * ch], ch, out);
            return Ok(true);
        }
    }

    /// Seeks to `pos`; returns the position actually reached.
    pub fn seek(&mut self, pos: Duration) -> anyhow::Result<Duration> {
        let time = Time::try_from_secs_f64(pos.as_secs_f64()).unwrap_or(Time::ZERO);
        let seeked = self
            .reader
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| anyhow!("seek failed: {e}"))?;
        self.decoder.reset();
        self.skip_until = Some(seeked.required_ts);
        let reached = self
            .time_base
            .and_then(|tb| tb.calc_time(seeked.required_ts))
            .map(|t| Duration::from_secs_f64(t.as_secs_f64().max(0.0)))
            .unwrap_or(pos);
        Ok(reached)
    }
}

/// Appends `src` (interleaved, `ch` channels) to `out` as interleaved stereo.
fn mix_to_stereo(src: &[f32], ch: usize, out: &mut Vec<f32>) {
    match ch {
        1 => {
            out.reserve(src.len() * 2);
            for &s in src {
                out.push(s);
                out.push(s);
            }
        }
        2 => out.extend_from_slice(src),
        _ => {
            // Simple downmix: front L/R plus the rest folded in equally.
            out.reserve(src.len() / ch * 2);
            for f in src.chunks_exact(ch) {
                let extra: f32 = f[2..].iter().sum::<f32>() / (ch - 2) as f32 * 0.5;
                out.push((f[0] + extra) * 0.7);
                out.push((f[1] + extra) * 0.7);
            }
        }
    }
}

/// Converts a span in time-base ticks to frames at `rate`, rounded to nearest.
/// Without a time base, ticks are already frames.
fn ticks_to_frames(ticks: i64, tb: Option<TimeBase>, rate: u32) -> i64 {
    let Some(tb) = tb else { return ticks };
    let n = ticks as i128 * tb.numer.get() as i128 * rate as i128;
    let d = tb.denom.get() as i128;
    ((n + n.signum() * d / 2) / d) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_to_frames_identity_and_scaled() {
        let tb = |n, d| TimeBase::try_new(n, d);
        assert_eq!(ticks_to_frames(12345, tb(1, 44100), 44100), 12345);
        assert_eq!(ticks_to_frames(250, tb(1, 1000), 48000), 12000);
        assert_eq!(ticks_to_frames(90000, tb(1, 90000), 44100), 44100);
        assert_eq!(ticks_to_frames(777, None, 48000), 777);
        assert_eq!(ticks_to_frames(-250, tb(1, 1000), 48000), -12000);
        // 1 tick of 1/90000 at 44100 is 0.49 frames: rounds to 0, not up.
        assert_eq!(ticks_to_frames(1, tb(1, 90000), 44100), 0);
    }

    #[test]
    fn mix_mono_upmixes_to_both_channels() {
        let mut out = vec![9.0];
        mix_to_stereo(&[0.25, -0.5], 1, &mut out);
        assert_eq!(out, [9.0, 0.25, 0.25, -0.5, -0.5]);
    }

    #[test]
    fn mix_stereo_passes_through() {
        let mut out = Vec::new();
        mix_to_stereo(&[0.1, 0.2, 0.3, 0.4], 2, &mut out);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn mix_six_channels_folds_extras() {
        let (l, r) = (0.4, -0.2);
        let (c, lfe, sl, sr) = (0.3, 0.1, -0.6, 0.2);
        let frames = [l, r, c, lfe, sl, sr, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut out = Vec::new();
        mix_to_stereo(&frames, 6, &mut out);
        let extra = (c + lfe + sl + sr) / 4.0 * 0.5;
        assert_eq!(out.len(), 4);
        assert!((out[0] - (l + extra) * 0.7).abs() < 1e-6);
        assert!((out[1] - (r + extra) * 0.7).abs() < 1e-6);
        assert_eq!(&out[2..], [0.0, 0.0]);
    }
}
