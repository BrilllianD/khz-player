# App and UI logic tests, with the duplication that blocks them

Stop after current on `TrackEnded`, gapless pointer sync, `skip_failed`, the add `play` flag,
`delete_playlist` retargeting, resume fields and AUTO remember have no test (real Y32).
`sort_by` and `randomize` share 14 lines verbatim (real Y31). The search-term split and match
exist twice, the jump copy untested (real Y42, Y44). The time readout contract is inline in
`show` (real Y43). Extends tasks 71 and 73, does not replace them.

- Where: `src/app/events.rs:31,113,177`, `src/app/playlists.rs:88,208`,
  `src/app/persist.rs:34`, `src/app/eq.rs:22`, `src/playlist.rs:294,411`,
  `src/ui/jump_dialog.rs:16-53`, `src/ui/library_panel.rs:87-99`, `src/ui/main_panel.rs:70-78`
- Done when: the decision parts (`TrackEnded` choice, retarget, resume fields) are pure
  functions with a test each; `randomize` reuses the body of `sort_by`; one `terms(query)` and
  one `matches(terms, hay)` serve both dialogs with tests for `"КИНО звез"`, an empty query and
  201 matches; a pure `time_text(..)` has tests for stopped, remaining past one hour and unknown
  duration.
