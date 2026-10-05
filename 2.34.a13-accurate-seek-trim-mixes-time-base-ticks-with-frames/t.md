# A13: accurate-seek trim mixes time-base ticks with frames

src/audio/decoder.rs:146-150 subtracts pts in time-base ticks from a frame count. Wrong trim whenever time base != sample rate.

Fix: convert until - pts through time_base to frames. Test with a time base != rate.

## Plan (from PLAN_TODO.md, line hooks checked at v0.1.2)

Problem: after `seek`, `skip_until = seeked.required_ts` is in time-base ticks (`decoder.rs:192`). `next_frames` subtracts `packet.pts` and treats the difference as frames (`decoder.rs:145-151`). Right only when the time base is `1/sample_rate` (usually MP3, FLAC, WAV). MP4/AAC often uses 1/1000 or a 90 kHz base, and then the trim drops the wrong amount.

Change:
- Add a pure `fn ticks_to_frames(ticks: i64, tb: Option<TimeBase>, rate: u32) -> i64`. With a time base it returns `ticks * numer * rate / denom`, computed in i128 and rounded. Without one, ticks are already frames.
- In `next_frames`, compute `behind` with it, using `self.time_base` and the packet's rate (`buf.spec().rate()`).
- Leave `seek`'s `reached` alone: `calc_time` already converts correctly.

Tests (`decoder.rs` has no test module yet; add one):
- `ticks_to_frames_identity_and_scaled`: tb 1/44100 at 44100 gives the same value; tb 1/1000 at 48000 turns 250 ticks into 12000 frames; tb 1/90000 at 44100 turns 90000 ticks into 44100; `None` passes through; negative values stay negative.
- E7 (task 74) goes in the same module: `mix_to_stereo` extraction and tests.

Manual: seek inside an M4A/AAC file; the heard position matches the time display.

Release: 0.1.3 (audio pass A13 to A17).
