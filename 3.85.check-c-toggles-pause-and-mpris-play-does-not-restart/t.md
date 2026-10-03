# Check: C toggles pause and MPRIS play does not restart

Fix A9 (v0.1.1). While playing press C (pauses), press C again (resumes at the same position). While playing run `playerctl -p rmp play`: the track must not restart. X while playing still restarts (Winamp behaviour). Not run by Claude: needs the GUI and audio.
