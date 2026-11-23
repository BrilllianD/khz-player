# Session and persistence: wrong resume point, lost playlist edits, silent failures

The saved session is (list name, row index, position) with no check that the row is the playing
track: after the playing tab is deleted or a list is renamed, the wrong track resumes at a
foreign position (real Y22). `move_selected` and `reverse` drop `resume_at`, so playback
restarts from row 0 after the playing row was removed (real Y23). `dirty` is cleared before
`save_playlist`, so a transient failure loses the edit for good, even at exit; a name swap
inside one debounce window lands here through `UNIQUE(name)` (real Y27). A database that fails
to open or is refused as newer gives no toast, saves nothing all session and overwrites the
session keys in the config; failed `cfg.save()` and `delete_playlist` only warn (real Y29).

- Where: `src/app/persist.rs:16-45`, `src/app/playlists.rs:198-202,225-228`,
  `src/app/mod.rs:194-214`, `src/playlist.rs:339,403`, `src/library/db.rs:27,367`
- Done when: a session whose row path differs from `now.path` writes no `last_position_ms`;
  `pl(5)`, `set_current(2)`, `remove([2])`, select `[0]`, `move_selected(2)` keeps
  `peek_next` on `/m/3.mp3`; a playlist whose save failed stays dirty and is written by the exit
  save; each failure path pushes a toast and a run with `db = None` leaves the session keys
  alone.
