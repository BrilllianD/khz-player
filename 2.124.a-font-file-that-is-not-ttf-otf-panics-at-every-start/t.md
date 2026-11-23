# A font file that is not TTF/OTF panics at every start

`fonts::find` checks only `is_file()` and hands the bytes to egui; epaint 0.36.2 panics on
unparsable font data. The settings field saves any path per keystroke, and `fc-match` may
return a bitmap font, so the player can crash on every launch until `config.toml` is edited by
hand (real R3).

- Where: `src/fonts.rs:14-45`, `src/ui/settings.rs:312-315`
- Done when: `fonts::install` with a temp file containing "not a font", followed by one
  `ctx.run_ui` pass, does not panic and falls back to the default chain; the settings field
  shows that the file was rejected.
