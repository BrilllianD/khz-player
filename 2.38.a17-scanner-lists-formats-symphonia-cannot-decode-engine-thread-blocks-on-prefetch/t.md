# A17: scanner lists formats symphonia cannot decode; engine thread blocks on prefetch

Scanner accepts opus/wv/ape/mpc (src/library/tags.rs:11-13) but symphonia features cannot decode them; alac, aiff, caf, mkv, opt-simd not enabled. Also PrefetchNext opens the decoder synchronously on the engine thread (engine.rs:285), EQ denormals in silence (dsp.rs:83-89), the error callback logs on every xrun (output.rs:134).

Fix: align the extension list with enabled features; add the missing features; open prefetch off-thread; flush denormals; rate-limit xrun logs.

## Plan (from PLAN_TODO.md)

Files: `Cargo.toml`, `src/library/tags.rs:11-13`, `src/audio/dsp.rs`, `src/audio/output.rs:134`, `src/audio/shared.rs`, `src/audio/engine.rs:287-301`. Four commits.

### 2a. Formats
- `Cargo.toml` symphonia features: add `alac`, `aiff`, `caf`, `mkv`, `adpcm`, `mp1`, `mp2`, `opt-simd`. Keep the existing ones (already includes `id3v1`, `id3v2`, `ape` for metadata).
- `AUDIO_EXTENSIONS`: drop `opus`, `wv`, `ape`, `mpc`. Add `aif`, `aiff`, `aifc`, `caf`, `mka`, `mp2`, `mp1`. Keep `mp4` and `m4a` (ALAC in MP4 now works too).
- Check `lofty` reads tags for the added containers. If not, `tags::read` must still return a `Track` with just the path, never an error that drops the file.
- Opus stays unsupported (research item "libopus via a custom CodecRegistry"); add a line to the README format list.
- The next rescan drops `.opus` and similar files from the DB via `delete_unseen`. Intended; mention it in the CHANGELOG.
- Note the release binary size before and after in the commit body. If it grows more than 1 MB stripped, drop `mkv` and `caf` first.
- Test: `tags.rs` `is_audio_matches_decodable_set` checks every listed extension is in an explicit allow-list. Also synthesize an AIFF with the WAV writer pattern from `scanner::tests::write_wav` (an AIFF header is about 50 bytes) and check `Decoder::open` succeeds.

### 2b. EQ denormals
- Biquad state is f64 (`dsp.rs:83-89`). After a signal stops, `z1`/`z2` decay into subnormal range, and each multiply costs about 100x on x86 exactly when the CPU should idle.
- At the end of `Equalizer::process`, flush tiny states to zero for every filter and channel (`if z.abs() < 1e-30 { 0.0 }`): 40 compares per callback, nothing per sample.
- Test `eq_state_flushes_to_zero_in_silence`: a sine with all bands at +12 dB, then about 10 s of zeros in callback-sized blocks; every `z1`/`z2` is exactly `0.0`.

### 2c. Xrun logging (can share a commit with 2b)
- Add `xruns: AtomicU64` to `Shared`. The error callback increments it on `e.kind() == cpal::ErrorKind::Xrun` and logs nothing for xruns. Other kinds keep the warn (A15, task 36, handles them).
- The engine thread logs `debug!("{n} xruns")` in `check_progress`, at most every 10 s, when the counter moved.

### 2d. Prefetch off the engine thread
- `PrefetchNext` runs `Decoder::open` on the engine thread (`engine.rs:295`). A slow or network disk stalls `fill()`, and the ring holds only 0.3 s.
- The engine owns `prefetch_rx: Receiver<(u64, PathBuf, anyhow::Result<Decoder>)>` and `prefetch_gen: u64`. `prefetch(Some(path))` bumps the generation and spawns a short-lived `prefetch` thread that opens the decoder and sends `(gen, path, result)`. `prefetch(None)` bumps the generation and clears `next`.
- The main loop uses `crossbeam_channel::select!` over `rx` and `prefetch_rx`, so a finished open wakes an idle engine. Old-generation results are dropped.
- `Decoder` must be `Send` (symphonia 0.6 declares the boxed reader and decoder `Send`; confirm at compile time).
- If the current track hits EOF before the prefetch answers, the track ends normally (`TrackEnded`, app advances). Slower but correct; do not wait.
- Test `prefetch_result_from_old_generation_is_ignored` with `test_engine()`: `prefetch(Some(a))`, `prefetch(Some(b))`, deliver both results, `next` holds b.

Release: 0.1.3.
