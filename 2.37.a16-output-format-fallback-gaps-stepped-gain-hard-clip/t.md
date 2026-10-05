# A16: output format fallback gaps, stepped gain, hard clip

Format fallback misses I24/U24/I8/U8/F64 (output.rs:78-83), picks a rate without checking channel count (:45-56), probes each config twice (:115-137); volume/balance step per block (:221); hard clamp after EQ (:231).

Fix: add the format arms, per-sample gain ramp, soft clip.

## Plan (from PLAN_TODO.md)

Files: `src/audio/output.rs`, `src/audio/dsp.rs`.

Change:
- Config choice: walk `supported_output_configs()` once. Keep configs that contain the default rate. Score: 2 channels before more channels before 1, then format F32 > I32 > I24 > I16 > U16 > others. Build the attempt list in that order, with the default config as the last resort.
- Formats: add arms for `I8`, `I24`, `U8`, `U24`, `F64`, `I64`, `U64`. All implement `SizedSample + FromSample<f32>` in cpal 0.18 (`sample_format.rs`).
- No double build: create the ring, the tap and the `CallbackState` inside the attempt loop. They are cheap; a failed attempt drops them. `open` returns the ends that belong to the stream that succeeded. Delete the no-op probe.
- Gain ramp: `CallbackState` keeps `gain: (f32, f32)` and moves it toward the `channel_gains(...)` target per frame with a step sized for about 10 ms, the same shape as the existing fade logic. Put the ramp in a small pure helper so it can be tested.
- Soft clip: pure `dsp::soft_clip(x: f32) -> f32`. Identity for |x| <= 0.9; above that `0.9 + 0.1 * tanh((|x| - 0.9) / 0.1)` with the sign kept, so it is bounded by 1.0, continuous and monotonic. Apply it after the volume instead of `clamp`.

Tests:
- `dsp.rs` `soft_clip_identity_below_knee_bounded_above`, also checking monotonicity on a sweep.
- E8 (task 75) `CallbackState::fill` tests live next to this change.

Manual: drag VOL fast while music plays and listen for zipper noise. EQ all +12 dB at preamp 0: distortion should be rounder, without crackle. The startup log line `audio output: ...` shows the chosen format.

Risk: config scoring can pick a different config than 0.1.2 on some hardware. The startup log shows the choice; the default config stays the last attempt so it never does worse.

Overlaps B6 (task 56): this covers the gain ramp and soft clip; the dB taper stays in B6. Release: 0.1.3.
