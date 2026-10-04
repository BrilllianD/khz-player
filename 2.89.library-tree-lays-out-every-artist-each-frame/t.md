# Library tree lays out every artist each frame

Found during check 86, measured by the user while playing: about 20 % CPU with show_library = true, about 13 % with the panel closed (275 artists / 1218 tracks, Ryzen 7 4700U). So the panel costs about 7 points. library_panel.rs tree() uses ScrollArea::show and builds a CollapsingHeader for every artist on every frame; while playing the UI repaints at 30 fps. Fix: flatten the visible rows (artists, open albums, tracks) and draw them with show_rows, like the playlist.
