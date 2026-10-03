# Check: skipping tracks does not write the playlist DB

Fix A2 (v0.1.1). Run `inotifywait -m ~/.local/share/rmp/` and skip tracks with B/Z or double-click. Expected: no library.db / library.db-wal writes from track changes; writes only after real playlist edits. Not run by Claude: needs the GUI.
