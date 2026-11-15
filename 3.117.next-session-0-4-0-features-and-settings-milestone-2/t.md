# Next session: 0.4.0 features and settings (milestone 2)

Plan for the next session. Outline: ~/.claude/plans/khz-0.4.0-outline.md (11 slices, 18 tasks, sum W 45), global constraints ~/.claude/plans/khz-0.4.0-outline-constraints.md (C1-C11: config fields, settings sections, shortcuts, ReplayGain contract, volume law, cover, notification, layout, gates, pulse packaging), code map ~/.claude/plans/khz-0.4.0-outline-map.md, brief ~/.claude/plans/khz-0.4.0-brief.md. Roadmap: ~/.claude/plans/make-big-plan-wise-meteor.md.

User requirement: every 0.4.0 feature gets settings-window configuration where it makes sense (persisted in config.toml, defaults, clamping, config tests).

User decisions (2026-11-15): U1 ReplayGain stored in the DB (schema v6, mtime = -1, needs_rescan at startup; refuses_newer_schema moves to 7). U2 output device list via the cpal pulseaudio feature, ALSA fallback; re-run the 0.3.1 recovery checks on the pulse host. U3 50 dB linear-in-dB volume taper. U4 LCD digits fixed, no toggle. U5 sleep timer ends with a fixed 10 s fade then pause; only sleep_minutes is a setting. U6 in-app cover in the spectrum row, show_cover off by default. U7 track-change notifications off by default.

Run per slice in outline order: scout map, designer slice plan, implementers one task each, one reviewer per slice, fix wave, scoped re-review; designers of slice K+1 overlap K unless it depends on K. Release 0.4.0 after the 11 manual checks in the outline (includes the v5 to v6 migration check and the pulse-host recovery checks). Follow-ups already filed: 115 (in slice 6), 116 (default-sink follow, out of scope).
