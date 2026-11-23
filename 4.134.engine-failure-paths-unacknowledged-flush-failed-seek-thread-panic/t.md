# Engine failure paths: unacknowledged flush, failed seek, thread panic

When the callback does not ack a flush within `FLUSH_TIMEOUT`, the engine pushes new audio that
the late `finish_flush` then discards, leaving `pushed_total` ahead of `consumed`; `TrackEnded`
then waits for a spurious reopen (real Y1). `seek` flushes before `decoder.seek`, and on `Err`
the clock is not rebased, so the display lags the audio; MPRIS `SetPosition` and `Resume { at }`
reach this path unclamped (real Y2). There is no `catch_unwind` around decoder calls: a dead
engine thread drops every command silently while the UI shows "Playing" (real Y7).

- Where: `src/audio/engine.rs:233-250,379-397,439,461`, `src/audio/output.rs:495-498`,
  `src/audio/mod.rs:108,166-168`, `src/app/events.rs:311`, `src/app/mod.rs:308`
- Done when: a test that acks the flush only after `fill` has pushed ends with exactly one
  `TrackEnded` and no `device_lost`; after a failed `Command::Seek` the position equals the
  frames pushed from the decoder's resume point, or nothing was flushed; after the engine thread
  ends, `AudioHandle::send` or a poll reports a failure.
