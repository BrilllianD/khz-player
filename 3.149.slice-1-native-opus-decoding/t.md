# Slice 1: Native Opus decoding

Part C slice 1 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 1'); constraints: outline-constraints.md next to it.

Boundary: `src/audio` (plus `src/library/tags.rs` extension list, `Cargo.toml`, `scripts/package.sh`)
Depends on: none
1. Opus and Opus-in-WebM through the engine, W = 4 (files 5: `Cargo.toml`,
   `src/audio/decoder.rs`, `src/library/tags.rs`, `scripts/package.sh`, `.github/workflows/ci.yml`
   → 2; +1 shared contract `AUDIO_EXTENSIONS` used by scanner, adder and dialog; +1 new test
   infrastructure, the ffmpeg fixture helper). Scope: adapter dependency, `mkv` feature, own
   `CodecRegistry`, `opus`/`webm` extensions, gapless and seek tests on generated fixtures,
   `cmake` in the package image. Consumes nothing. Produces `decoder::codecs()` and
   `fixtures::ffmpeg_fixture`.
ΣW = 4. Produces for later slices: `.opus`/`.webm` play through the engine; the fixture helper.
Must verify (unverified points): gapless through the adapter (pre-skip and end trim give the
sample count ffmpeg wrote); seek accuracy (landed position within one Opus frame, 20 ms); Opus
inside WebM demuxes through the `mkv` reader; lofty reads tags from `.opus`; `cmake` present on
the CI runner (ubuntu-22.04 images ship it) and added to the `ubuntu:22.04` package image.
