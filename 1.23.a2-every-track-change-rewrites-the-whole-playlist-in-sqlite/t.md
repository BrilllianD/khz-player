# A2: every track change rewrites the whole playlist in SQLite

Playlist::set_current sets dirty (src/playlist.rs:388), so each play_index runs save_playlist (DELETE + INSERT n rows, src/library/db.rs:243-249) on the UI thread.

Fix: set_current must not mark the list dirty. The current index is persisted through cfg.last_track_index, which on_track_started already covers via mark_cfg.
