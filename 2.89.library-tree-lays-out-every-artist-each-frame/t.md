# Library tree lays out every artist each frame

Found during check 86: about 20 % CPU while playing (14 % on the UI thread) with show_library = true and 275 artists / 1218 tracks. library_panel.rs tree() uses ScrollArea::show and builds a CollapsingHeader for every artist on every frame; while playing the UI repaints at 30 fps. Suspected cause, not yet confirmed by measuring with the panel closed. Fix: flatten the visible rows (artists, open albums, tracks) and draw them with show_rows, like the playlist.
