# E1: split src/app.rs

app.rs is about 1000 lines. Split at the seam near the event handlers into app/{mod,playback,playlists,eq,persist,events}.rs; move UI-only fields into a UiState struct.
