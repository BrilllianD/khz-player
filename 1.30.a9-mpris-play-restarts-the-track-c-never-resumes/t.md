# A9: MPRIS Play restarts the track; C never resumes

MprisAction::Play calls play(), which restarts a playing track (src/app.rs:773). The C key only pauses (src/shortcuts.rs:30), Winamp's C toggles.

Fix: MPRIS Play is a no-op while playing; new Action::PauseToggle on C (playing -> pause, paused -> play, stopped -> nothing). README key table. Shortcut tests for C and for Alt+L ordering.
