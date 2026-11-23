# Keyboard reach: library, playlist tabs, EQ

No shortcut focuses the library search, switches playlists or moves in the library tree; EQ
presets and sliders need the mouse; the jump dialog has no PgUp, PgDn, Home, End (real Y40).

- Where: `src/ui/library_panel.rs:176-180,291-427`, `src/ui/playlist_panel.rs:57-63`,
  `src/ui/eq_panel.rs:22-87`, `src/shortcuts.rs:8-45`, `src/ui/jump_dialog.rs:33-53`
- Done when: `shortcuts::collect` has bindings for library search focus and next and previous
  playlist (tested), the library tree moves a cursor with arrows and adds with Enter, and README
  lists them.
