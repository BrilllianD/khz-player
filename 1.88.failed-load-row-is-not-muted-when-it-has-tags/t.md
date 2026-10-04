# Failed-load row is not muted when it has tags

Found in check 83. playlist_panel.rs mutes a row only when title and duration_ms are both None, so a track added from the library (with tags) that fails to load keeps the normal colour. Since 706f10c the failed-load set is checked inside that tag condition. Fix: a path in app.failed is always muted; the stat() fallback stays limited to untagged rows.
