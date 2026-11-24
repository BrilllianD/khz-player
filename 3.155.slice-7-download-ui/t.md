# Slice 7: Download UI

Part C slice 7 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 7'); constraints: outline-constraints.md next to it.

Boundary: `src/ui`
Depends on: Slice 6
1. Prompt checkbox, ADD item, footer label, jobs modal, settings section, info row, W = 3
   (files 5: `src/ui/mod.rs`, `src/ui/playlist_panel.rs`, `src/ui/settings.rs`,
   `src/ui/info_dialog.rs`, `src/ui/main_panel.rs` or wherever the footer lives → 2; +1 shared
   state, the modal set fed to `modal_open`). Consumes the App download API and
   `tags::source_url`. Produces nothing for later slices.
ΣW = 3.
