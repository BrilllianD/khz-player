# E9: engine gapless tests on synthetic WAVs

Gapless A->B across rates (exact frame count, Advanced timing), TrackEnded without prefetch, SeekRel clamp.

## Plan (from PLAN_TODO.md)

Last step of the audio pass, after A13 to A17. Engine tests on synthetic WAVs using `test_engine()` and a fake consumer that drains the ring and advances `consumed`:
- Gapless A (8000 frames at 8 kHz) to B (8000 frames at 16 kHz): total frames pushed equal `len(A) + len(B)` resampled (within resampler rounding). `Advanced(B)` fires when `consumed` crosses the mark, not earlier.
- `TrackEnded` without prefetch fires once, after the ring drains.
- `SeekRel` past the end clamps to `duration - 0.5 s`. A24 changes this to Next later; the test documents today's behaviour and gets updated then.
