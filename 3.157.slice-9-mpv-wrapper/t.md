# Slice 9: mpv wrapper

Part C slice 9 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 9'); constraints: outline-constraints.md next to it.

Boundary: `src/video/` (plus `Cargo.toml`, CI and package build deps)
Depends on: none
1. `VideoPlayer` over libmpv2, feature `video`, W = 5 (files 6: `src/video/mod.rs` new,
   `src/video/events.rs` new, `src/lib.rs`, `Cargo.toml`, `.github/workflows/ci.yml`,
   `scripts/package.sh` → 2; +2 new interface; +1 new test infrastructure, a real-libmpv test
   on a generated file that is skipped without ffmpeg). Scope: options, event thread mapping
   mpv events and observed `pause`/`time-pos`/`duration`/end-file/shutdown to `VideoEvent`, the
   commands, the stub without the feature, a pure event-mapping test.
ΣW = 5. Produces: `video::{VideoPlayer, VideoEvent}`.
Must verify: `libmpv2` 7.0.1 builds without default features against mpv 0.41 here; Ubuntu
22.04 ships only `libmpv.so.1` / mpv API 1.x, which `libmpv2` rejects. If confirmed, the CI
test job (which must link libmpv once this slice lands) moves to `ubuntu-24.04` with
`libmpv-dev`, and a `--no-default-features` build stays in CI. The tarball question is a user
decision, needed before this slice's implementer: either the `ubuntu:22.04` image builds
without `video`, or the image moves to 24.04 (newer glibc floor). Assumption until answered:
the tarball builds without `video` and the README says so. Also verify mpv's volume curve
(cubic softvol) against the engine's volume taper, so `set_volume` sounds the same.
