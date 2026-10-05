# E8: tests for CallbackState::fill

Flush ack, pause fade, underrun zeros, consumed count, tap overflow, int conversion, using plain ring ends (output.rs:151).

## Plan (from PLAN_TODO.md)

Write these with A16 (task 37). `CallbackState::new` is private but in the same module. Cover:
- flush ack clears the ring and the flag;
- paused with fade 0 outputs silence and consumes nothing;
- underrun zero-fills and counts `consumed` correctly;
- the tap gets a mono post-EQ signal and `tap_overflow` counts the overflow;
- `fill::<i16>` converts full scale to `i16::MAX` (after the soft clip, 1.0 maps to about 0.9999);
- a volume step ramps over about 480 frames at 48 kHz, not in one frame.
