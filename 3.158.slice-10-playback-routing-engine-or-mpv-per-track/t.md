# Slice 10: Playback routing, engine or mpv per track

Part C slice 10 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 10'); constraints: outline-constraints.md next to it.

Boundary: `src/app`
Depends on: Slice 5, Slice 8, Slice 9
1. Per-track backend with both paths behind the transport seam, W = 5 (files 5:
   `src/app/transport.rs`, `src/app/playback.rs`, `src/app/events.rs`, `src/app/mod.rs`,
   `src/mpris.rs` or its glue in `events.rs` → 2; +2 new interface `Backend`/`backend_for`;
   +1 more than ~10 files to read: playback, events, mod, playlists, transport, persist,
   `playlist.rs`, `mpris.rs`, `audio/mod.rs`, `audio/shared.rs`, `video/`). Scope: `backend_for`,
   `VideoPlayer` owned by `App` (created lazily on the first video track), stop the other
   backend before a load, drain `VideoEvent` in `logic`, end of video goes through the
   playlist advance rules, UI and MPRIS position, seek and status on both paths, volume and
   device mirrored, a video track at startup resume, no prefetch for video. Tests on
   `backend_for` and on the advance decision.
ΣW = 5. Produces: `Backend`, `App::video` (`Option<VideoPlayer>`).
