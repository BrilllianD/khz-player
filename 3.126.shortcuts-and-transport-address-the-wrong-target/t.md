# Shortcuts and transport address the wrong target

Del, Enter, Q and Ctrl+A sit in `BINDINGS`, so they act on the playlist while it is hidden or
unfocused (abs Y16). Ctrl+V misses when Ctrl is released before V (abs Y17). Toggle bindings
fire on key repeat: a held Ctrl+D thrashes the zoom, a held Del removes a row per repeat (real
Y28). Enter and Alt+3, Q, and X each pick "the row the user means" by a different rule (abs
Y12). The Pause button pushes `TogglePlay` under the hover text "Pause (C)" while the C key
does nothing when stopped (real Y33).

- Where: `src/shortcuts.rs:69-72,108-124`, `src/app/mod.rs:359-372,418-436`,
  `src/app/playback.rs:135`, `src/app/playlists.rs:254-266`, `src/ui/main_panel.rs:265-266`
- Done when: `press(false, NONE, Key::Delete)` returns no `RemoveSelected`; Ctrl down, V down,
  Ctrl up, V up yields `[StopAfterCurrent]`; `collect` with `repeat: true` on Ctrl+D and Del
  returns nothing while ArrowUp still returns `VolumeDelta`; one `Playlist` method returns the
  target row for all four commands; the Pause button and the C key dispatch the same action.
