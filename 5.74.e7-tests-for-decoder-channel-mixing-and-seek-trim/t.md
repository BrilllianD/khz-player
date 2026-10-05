# E7: tests for decoder channel mixing and seek trim

Mono upmix, 6ch downmix weights, seek trim with time base != rate (A13).

## Plan (from PLAN_TODO.md)

In the new `decoder.rs` test module (added with A13, task 34):
- Extract the channel mapping from `next_frames` into `fn mix_to_stereo(src: &[f32], ch: usize, out: &mut Vec<f32>)`.
- Test mono upmix (L = R = s).
- Test the 6-channel downmix weights: fold-in of 0.5 times the average of the extra channels, then times 0.7.
- The seek-trim part is covered by `ticks_to_frames_identity_and_scaled` in A13.
