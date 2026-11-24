# Slice 8: Transport seam (audit 142, part 2)

Part C slice 8 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 8'); constraints: outline-constraints.md next to it.

Boundary: `src/app` (plus the two `src/ui/main_panel.rs` call sites)
Depends on: none
1. One transport entry point, W = 4 (files 6: `src/app/transport.rs` new, `src/app/mod.rs`,
   `src/app/events.rs`, `src/app/playback.rs`, `src/app/playlists.rs`, `src/ui/main_panel.rs`
   → 2; +2 new interface `t_*` and `App::position`). Scope: move every `audio.send` and
   `audio.position` behind the methods in the constraints file, no behaviour change, the
   existing tests stay green. Produces the transport seam.
ΣW = 4. Produces: `App::{t_play, t_pause, t_stop, t_seek, t_seek_rel, t_load, position}`.
