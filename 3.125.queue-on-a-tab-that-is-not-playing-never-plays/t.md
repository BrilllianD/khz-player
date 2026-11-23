# Queue on a tab that is not playing never plays

Q and Ctrl+Enter in the jump dialog queue rows of the active playlist, while `next()` and
`TrackEnded` pop the queue of the playing playlist. Rows queued on another tab get `[n]`
markers and are never played, against README "played next, in order" (real R5).

- Where: `src/app/playlists.rs:254-272`, `src/app/playback.rs:109-114`
- Done when: with `playing_list = 0`, `active = 1` and row 3 of list 1 queued, `next()` loads
  list 1 row 3, or the queue action is refused with a toast; a test pins the chosen rule.
