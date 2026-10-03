# A1: removing the playing track restarts from row 1

Playlist::remove sets current = None when the playing row is removed (src/playlist.rs:102-108). peek_next with no current returns Some(0) (src/playlist.rs:315, shuffle :305), so after Del on the playing row playback continues gaplessly into row 1. Same for crop, remove missing, dedup (src/ui/playlist_panel.rs:302-322).

Fix: remember the removed row's position in a private resume_at field and continue from there in peek_next/next/prev. Clear it on set_current, clear, sort, reverse, randomize. Also reset the stale anchor in sort_by, reverse and randomize.
