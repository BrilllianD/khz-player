# Non-finite values reach the audio output as full-scale noise

`f32::clamp` passes NaN. A NaN volume or balance (MPRIS `Volume` set, forwarded raw) makes
`channel_gains` return NaN and `approach` run the gain down without bound, which `soft_clip`
turns into ±1 on every sample; the NaN is also written to `config.toml` (real R4). The same
gap lets `nan` in `eq_presets.toml` reach the EQ coefficients (real Y19), and one NaN or Inf
sample poisons the filter state until the next flush because `flush_tiny` keeps NaN (real Y6).

- Where: `src/audio/dsp.rs:92-98,202,225-243`, `src/audio/output.rs:468-477`,
  `src/app/playback.rs:213-229`, `src/mpris.rs:335`, `src/app/events.rs:314`,
  `src/eq_presets.rs:65-66`, `src/audio/shared.rs:107-113`
- Done when: `approach(0.5, f32::NAN, 0.1)` stays in [0, 1]; `channel_gains(NAN, NAN)` is
  finite; `MprisAction::Volume(f64::NAN)` leaves `cfg.volume` finite; a preset with `preamp=nan`
  parses to finite values; one NaN frame through `Equalizer::process` followed by a sine gives
  finite output within one block.
