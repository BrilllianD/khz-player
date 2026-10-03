# A10: stat() per visible playlist row per frame

The muted row colour calls t.path.exists() for every visible row every frame (src/ui/playlist_panel.rs:146).

Fix: test title/duration first and the failed set from A7 before exists(), so stat only runs for bare untagged rows.
