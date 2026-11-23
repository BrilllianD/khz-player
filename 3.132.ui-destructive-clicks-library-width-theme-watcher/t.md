# UI: destructive clicks, library width, theme watcher

Deleting a playlist, "Clear playlist" and a double-click on a library track (`Act::Replace`)
wipe content at once with no confirmation or undo (real Y41). The library width is saved on
frames where the zoom has doubled but the window has not grown, so a wide library is saved
squeezed (real Y34). The theme watcher never starts when the Omarchy directory is absent at
launch, even after "follow theme" is ticked, and dies silently when the directory is re-created
(real Y37).

- Where: `src/ui/playlist_panel.rs:69-70,434-435,510-511`, `src/ui/library_panel.rs:387-388`,
  `src/ui/mod.rs:84-87`, `src/theme_watch.rs:18-34`, `src/app/mod.rs:347-353`
- Done when: deleting or clearing a non-empty playlist confirms or offers undo and library
  double-click is documented or appends; with `library_w = 700`, toggling Ctrl+D twice leaves
  700 in the saved config; a test with an injected temp directory sees `changed` fire after a
  write and again after the directory is re-created.
