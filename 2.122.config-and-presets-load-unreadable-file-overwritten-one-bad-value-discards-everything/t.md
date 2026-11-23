# Config and presets load: unreadable file overwritten, one bad value discards everything

`load_from` and `load_user_from` treat every read error other than a parse error (non-UTF-8
bytes, `EACCES`, `EISDIR`) as "no file": defaults are used, nothing is set aside, and the next
save overwrites the user's file (real R2). A single bad value fails the whole parse and resets
every setting, `library_roots` included, which feeds the scanner data loss above (abs Y1). A
non-UTF-8 library root or font path makes every later config save fail with only a warn (abs
Y2).

- Where: `src/config.rs:87,214-232`, `src/eq_presets.rs:83-91`, `src/app/persist.rs:43-45`
- Done when: a config with an invalid UTF-8 byte returns `Some(err)` and leaves
  `config.toml.broken` (same for presets), a missing file still returns `None`; a config with
  one bad key loads every other key and reports the bad one; a config holding a non-UTF-8 path
  saves and reloads, or the path is refused with a toast when added.
