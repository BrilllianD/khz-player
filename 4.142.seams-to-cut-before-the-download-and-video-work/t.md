# Seams to cut before the download and video work

Structural findings the planned yt-dlp job and libmpv routing will hit. There is no shared
background-job contract: seven spawn sites, seven lifecycles, no cancel (abs Y11). The engine is
driven through `Command` messages and through direct stores into public `Shared` atomics from
about ten `App` sites and from the UI (abs Y8). Modals have four state shapes and a hand-written
shortcut gate (abs Y14). Settings controls write `cfg` directly in three styles (abs Y15). Cover
lookup has two pipelines with different staleness rules (abs Y13).

- Where: `src/library/scanner.rs:25`, `src/library/adder.rs:91`, `src/dialog.rs:33`,
  `src/audio/mod.rs:67-72`, `src/audio/shared.rs:29-60`, `src/ui/main_panel.rs:64,240`,
  `src/app/mod.rs:479-494`, `src/ui/settings.rs:133-291`, `src/ui/eq_panel.rs:22-33`,
  `src/app/events.rs:127-140`, `src/ui/cover.rs:68-88`
- Done when: scanner and adder run on one job type with progress, a terminal event, a cancel
  handle and end-on-disconnect; `src/ui` has no `audio.send` or `audio.shared` and engine-only
  atomics are not visible outside `audio`; one `App::modal_open()` feeds the gate;
  `grep -cE "app\.cfg\.[a-z_.]* *=[^=]|&mut app\.cfg" src/ui/*.rs` returns 0; `art::find` has
  one call site outside tests.
