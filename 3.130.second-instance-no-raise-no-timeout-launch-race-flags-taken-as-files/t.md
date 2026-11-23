# Second instance: no raise, no timeout, launch race, flags taken as files

Forwarding files to the running player does not raise its window, against CHANGELOG 0.3.0, and
`OpenUri` never starts playback (real Y25). `forward_to_running` uses a bare
`Connection::session()`, so it hangs forever against a stopped owner (real Y45). The check and
the name claim are separate steps, so two quick launches both run as full players on one config
and one database (real Y46). Every argument is a path: `--version` opens the player or is
forwarded as a file (abs Y19). The forward mapping and the metadata build have no tests (real
Y47).

- Where: `src/main.rs:14-15`, `src/mpris.rs:202-229,251,262,347,372-382`,
  `src/app/events.rs:320`, `src/app/mod.rs:222`, `CHANGELOG.md:42-44`
- Done when: `khz-player a.mp3` against a running instance raises the window and follows a
  stated play rule; no bare `Connection::session()` is left in `src/`; two launches 100 ms apart
  leave one process or the loser exits with a message; `khz-player --version` prints the version
  and exits 0 without a window; tests map a relative path to `file://<cwd>/…` and build
  `Metadata` from a track with no artist and no length.
