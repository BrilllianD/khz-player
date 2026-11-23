# UI per-frame work: jump filter and file stats

The jump dialog lowercases every track of the playlist on every frame (real Y38). Playlist rows
without a title call `path.exists()` per row per frame on the UI thread, which stalls on a
network mount (real Y39).

- Where: `src/ui/jump_dialog.rs:16-32`, `src/ui/playlist_panel.rs:185`, `src/playlist.rs:243`
- Done when: hits are recomputed only when the query or the playlist changes; the row colour
  comes from a cached flag and `rows` makes no syscalls.
