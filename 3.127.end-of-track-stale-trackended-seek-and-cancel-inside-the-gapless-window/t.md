# End of track: stale TrackEnded, seek and cancel inside the gapless window

`Event::TrackEnded` carries no identity, so one queued just before the user picks a track makes
the app advance past that pick (real Y8, abs Y9). In the last ~0.3 s of a track the decoder is
already the gapless successor: Seek, SeekRel and Play act on it with the predecessor's position
(real Y3), and `PrefetchNext(None)` (stop after current, gapless off, row removed) cannot revoke
it, so stop-after stops one track late (real Y4). After `LoadFailed` the engine's replay target
and `App::now` name different tracks (abs Y10).

- Where: `src/audio/mod.rs:57-60`, `src/audio/engine.rs:282,292-300,345-350,376-378,443-447,
  587,644`, `src/app/events.rs:29-36,176-182,203`, `src/app/playback.rs:138,147`
- Done when: events carry the path or a load generation and the app drops a mismatch (test:
  `play_index(.., 5)` then a stale `TrackEnded` leaves row 5 current); with a pending
  transition, `SeekRel(-1000)` emits `Seeked` for the predecessor and `PrefetchNext(None)` ends
  it with `TrackEnded` and no `Advanced`; after `LoadFailed` both sides name the same path or
  none.
