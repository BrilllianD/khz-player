# Library and files nits

The blue library items of the realization note: ghost row for a file deleted mid-scan,
`mtime = -1` rows under an unmounted root forcing a rescan at every start, CP1251 heuristic on
short Latin-1, unchecked ReplayGain range, M3U entries that are directories, `#EXTINF`
overflow, WAL before busy timeout, adder swallowing errors and lacking cancel, `write_atomic`
tmp handling, art cache growth, `.broken` overwritten, duplicated `track_for` and test helpers.

- Where: `src/library/scanner.rs:117`, `src/library/db.rs:67,70,137`,
  `src/library/tags.rs:35,61,101`, `src/library/adder.rs:30,47,66,71,100,101`, `src/m3u.rs:34`,
  `src/config.rs:265,279`, `src/library/art.rs:96,101`
- Done when: each item in the "Library and files" blue list of the realization note is fixed or
  struck with a reason.
