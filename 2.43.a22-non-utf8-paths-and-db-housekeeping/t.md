# A22: non-UTF8 paths and DB housekeeping

Paths are stored lossily (src/library/db.rs:84-86, scanner.rs:69), so non-UTF8 files never match and are rescanned each time. Also clear_seen uses substr (full scan, db.rs:179), idx_tracks_artist_album is unused, and there is no guard for user_version > 4 (db.rs:70-81).

Fix: store paths as blobs (OsStr bytes) in a migration; range query instead of substr; drop the index; refuse newer schema.
