//! Streaming stereo resampler on top of rubato's synchronous FFT resampler.
//! Bypassed when the track rate equals the output rate.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler as _};

const CH: usize = 2;
const CHUNK: usize = 1024;

pub struct Resampler {
    inner: Option<Inner>,
}

struct Inner {
    rs: Fft<f32>,
    ratio: f64,
    pending: Vec<f32>,
    chunk_out: Vec<f32>,
    /// Output frames still to drop to compensate the resampler delay.
    to_drop: usize,
    in_total: u64,
    out_total: u64,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> anyhow::Result<Self> {
        if in_rate == out_rate {
            return Ok(Self { inner: None });
        }
        let rs = Fft::<f32>::new(
            in_rate as usize,
            out_rate as usize,
            CHUNK,
            CH,
            FixedSync::Input,
        )?;
        let chunk_out = vec![0.0; rs.output_frames_max() * CH];
        let to_drop = rs.output_delay();
        Ok(Self {
            inner: Some(Inner {
                ratio: out_rate as f64 / in_rate as f64,
                rs,
                pending: Vec::with_capacity(CHUNK * CH * 2),
                chunk_out,
                to_drop,
                in_total: 0,
                out_total: 0,
            }),
        })
    }

    #[cfg(test)]
    pub fn is_bypass(&self) -> bool {
        self.inner.is_none()
    }

    pub fn reset(&mut self) {
        if let Some(i) = &mut self.inner {
            i.rs.reset();
            i.pending.clear();
            i.to_drop = i.rs.output_delay();
            i.in_total = 0;
            i.out_total = 0;
        }
    }

    /// Appends resampled frames for `input` (interleaved stereo) to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> anyhow::Result<()> {
        let Some(i) = &mut self.inner else {
            out.extend_from_slice(input);
            return Ok(());
        };
        i.pending.extend_from_slice(input);
        i.in_total += (input.len() / CH) as u64;
        let mut start = 0;
        loop {
            let need = i.rs.input_frames_next();
            if (i.pending.len() - start) / CH < need {
                break;
            }
            i.run(start, need, None, out)?;
            start += need * CH;
        }
        i.pending.drain(..start);
        Ok(())
    }

    /// Flushes buffered input and the resampler tail at end of stream.
    pub fn finish(&mut self, out: &mut Vec<f32>) -> anyhow::Result<()> {
        let Some(i) = &mut self.inner else {
            return Ok(());
        };
        let expected = (i.in_total as f64 * i.ratio).round() as u64;
        let mut guard = 0;
        while i.out_total < expected && guard < 64 {
            let need = i.rs.input_frames_next();
            let take = (i.pending.len() / CH).min(need);
            let idx = Indexing::new().partial_len(take);
            let before = out.len();
            i.run(0, need, Some(&idx), out)?;
            i.pending.drain(..take * CH);
            // Trim anything past the expected length.
            if i.out_total > expected {
                let extra = (i.out_total - expected) as usize;
                let new_len = out.len().saturating_sub(extra * CH).max(before);
                out.truncate(new_len);
                i.out_total = expected;
            }
            guard += 1;
        }
        self.reset();
        Ok(())
    }
}

impl Inner {
    fn run(
        &mut self,
        start: usize,
        need: usize,
        indexing: Option<&Indexing>,
        out: &mut Vec<f32>,
    ) -> anyhow::Result<()> {
        let avail = (self.pending.len() - start) / CH;
        let frames_in = need.min(avail);
        let input = InterleavedSlice::new(&self.pending[start..], CH, frames_in)?;
        let out_frames = self.chunk_out.len() / CH;
        let mut output = InterleavedSlice::new_mut(&mut self.chunk_out, CH, out_frames)?;
        let (_, n_out) = self.rs.process_into_buffer(&input, &mut output, indexing)?;
        let drop = self.to_drop.min(n_out);
        self.to_drop -= drop;
        out.extend_from_slice(&self.chunk_out[drop * CH..n_out * CH]);
        self.out_total += (n_out - drop) as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypass_same_rate() {
        let mut r = Resampler::new(48000, 48000).unwrap();
        assert!(r.is_bypass());
        let mut out = Vec::new();
        r.process(&[0.1, 0.2, 0.3, 0.4], &mut out).unwrap();
        assert_eq!(out, vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn length_and_pitch_44k_to_48k() {
        let mut r = Resampler::new(44100, 48000).unwrap();
        let frames = 44100;
        let freq = 1000.0f32;
        let mut input = Vec::with_capacity(frames * 2);
        for n in 0..frames {
            let s = (2.0 * std::f32::consts::PI * freq * n as f32 / 44100.0).sin() * 0.5;
            input.push(s);
            input.push(s);
        }
        let mut out = Vec::new();
        // Feed in odd-sized pieces like a decoder would.
        for piece in input.chunks(1152 * 2) {
            r.process(piece, &mut out).unwrap();
        }
        r.finish(&mut out).unwrap();
        assert_eq!(out.len() / 2, 48000);
        // Count zero crossings on the left channel in the middle: 1 kHz => ~2 per ms.
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let mid = &left[4800..43200];
        let crossings = mid
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        let expected = 2.0 * freq * (mid.len() as f32 / 48000.0);
        assert!((crossings as f32 - expected).abs() < 4.0, "{crossings} vs {expected}");
    }
}
