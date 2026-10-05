# A14: mid-stream sample-rate change not handled

ResetRequired is treated as EOF and a rate change is ignored (src/audio/decoder.rs:114,140, src/audio/engine.rs:334-336).

Fix: compare the decoder rate to the resampler input rate in fill and rebuild the resampler on mismatch; handle ResetRequired by resetting the decoder.

## Plan (from PLAN_TODO.md)

Files: `src/audio/decoder.rs:111-140`, `src/audio/resample.rs`, `src/audio/engine.rs:328-368`.

Problem: when a packet's rate differs, `next_frames` updates `info.sample_rate` (`:137-140`), but the engine keeps feeding the old resampler, so the audio plays at the wrong pitch. `Error::ResetRequired` from `next_packet` is treated as end of stream (`:114`). Chained Ogg streams and some radio rips hit this, and the track is cut short.

Change:
- `Resampler` remembers `in_rate`/`out_rate` and gets `pub fn in_rate(&self) -> u32`.
- In `Engine::fill`, after `next_frames` returns `Ok(true)`: if `cur.decoder.info.sample_rate != cur.resampler.in_rate()`, first `cur.resampler.finish(&mut self.buf)` (emits the old-rate tail), then `cur.resampler = Resampler::new(new_rate, out_rate)?`, then process `scratch`.
- Decoder `ResetRequired`: re-select `reader.default_track(TrackType::Audio)`, rebuild the decoder with `make_audio_decoder` for the new params, update `track_id`, `time_base`, `info.sample_rate` and `info.channels`, then continue the loop. Return `Ok(false)` only when no audio track is left. Put this in a private `fn reset_track(&mut self) -> Result<()>`.
- Position: `track_start` is in output frames and the output rate does not change, so the position stays correct across the switch. Only `info.duration` may become stale; leave it.

Tests:
- `resample.rs` `rate_switch_keeps_length`: 1 s at 44100, `finish`, a new resampler, then 1 s at 32000. Output is 96000 frames at 48000, within ±2. Guards that finish-then-rebuild loses no frames.
- A chained Ogg file is hard to synthesize. Test ResetRequired with a real file only, `#[ignore]` in the `engine_real_file` style, using `RMP_TEST_CHAINED_OGG`.

Manual: `cat a.ogg b.ogg > chained.ogg` (a at 44.1 kHz, b at 48 kHz) and play it. The second part plays at the right pitch and does not stop early.

Risk: check that the symphonia 0.6 Ogg reader actually emits ResetRequired on chain boundaries. If it handles chains internally, this reduces to the rate-change part.

Release: 0.1.3.
