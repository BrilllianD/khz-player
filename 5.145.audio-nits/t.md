# Audio nits

The blue audio items of the realization note: EQ bands above 0.45·fs stacking, spectrum bins at
other rates, out rate published before the rebase, failed streams dropped inline, unbounded
join on drop, 200 Hz tick while paused, `work.resize` in the callback, silent decoder discards,
the sleep timer restoring `sleep_gain` after an ignored Pause, repeated test harness.

- Where: `src/audio/dsp.rs:37`, `src/audio/spectrum.rs:81`, `src/audio/engine.rs:162,386,420`,
  `src/audio/output.rs:152,158,439`, `src/audio/mod.rs:180`, `src/audio/decoder.rs:41,136,199`,
  `src/app/playback.rs:51`
- Done when: each item in the "Audio" blue list of
  `~/mem/projects/khz-player/notes/khz-player-realization-review.md` is fixed or struck with a
  reason.
