# Data contracts: Track.mtime sentinel, pub surface, track column list

`Track.mtime` is a file stamp and a state flag at once (`0` never read, `-1` stale, and `0`
again for a failed stat) (abs Y3). Every module in `src/lib.rs` is `pub`, so `dead_code` never
fires (abs Y4). `load_playlists` repeats the `tracks` column list and reads column 19 by index,
which a schema v7 column would shift (abs Y5).

- Where: `src/library/mod.rs:22`, `src/library/tags.rs:27-35`, `src/app/playlists.rs:119`,
  `src/app/mod.rs:317,556`, `src/lib.rs:3-16`, `src/library/db.rs:336-350`
- Done when: `Track` carries an explicit metadata state and `grep -rn "mtime == 0" src` is
  empty; only what `main.rs` and `benches/perf.rs` use is `pub`; one constant builds both column
  lists and the extra column is read by name.
