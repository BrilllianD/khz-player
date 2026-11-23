# Audio tests: bad files, callback paths, rate changes, wall-clock asserts

`run` and `test_engine` each spell out all 25 `Engine` fields and already differ (real Y9).
Several tests assert under 5 ms or 200 ms of wall time or sleep 100 ms (real Y10). Zero-length,
truncated and corrupt files are never fed to `Decoder` (real Y11). The fade-in after a flush
ack, the pause ramp, mono downmix and surround zero-fill have no test (real Y12). The mid-track
rate-change branch and channel-count changes run only in an `#[ignore]` test (real Y13).

- Where: `src/audio/engine.rs:131,539,564,710,715,754,765,1067,1078,1345`,
  `src/audio/output.rs:471,479,483,757`, `src/audio/decoder.rs:122,132`
- Done when: one `Engine::new(..)` serves `run` and the tests; no `elapsed() <` bound below 1 s
  remains; a truncated WAV ends with `TrackEnded`, a zero-byte file gives `LoadFailed`, an
  injected read error gives `Error` then `TrackEnded`; tests assert the ramp after an ack and
  `fill` with 1 and 6 channels; a non-ignored test runs a gapless pair with different channel
  counts and a synthetic rate change.
