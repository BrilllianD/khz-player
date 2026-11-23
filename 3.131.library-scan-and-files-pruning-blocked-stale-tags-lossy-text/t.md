# Library scan and files: pruning blocked, stale tags, lossy text

One walk error (dangling symlink, `lost+found`) disables pruning of the whole root for good
(real Y16). Change detection is whole-second mtime plus size, so same-length edits with a
preserved time stay stale and nothing forces a re-read (real Y17). The CP1251 repair maps only
Russian letters (real Y15). M3U export and import lose non-UTF-8 path bytes (real Y14). Library
grouping does not trim, so "Artist" and "Artist " are two groups (real Y35). A scanner thread
that fails to spawn or panics leaves the scan marked as running for the whole session (real
Y26).

- Where: `src/library/scanner.rs:29,39-42,54,71,114`, `src/library/tags.rs:27-33,72-83`,
  `src/m3u.rs:89-92,110,161`, `src/ui/library_panel.rs:73`, `src/library/mod.rs:75-87`,
  `src/app/events.rs:216-250`
- Done when: a root with a dangling symlink and one deleted file reports `removed == 1`; a
  same-length rewrite with restored mtime gives `updated == 1` or a "re-read all tags" command
  exists; `decode_cp1251(b"\xb3\xbf\xba\x92")` returns "іїє’"; `m3u::write` then `m3u::read` of
  `/m/bad\xff.mp3` returns that path; artists `"Кино"`, `"Кино "`, `" кино"` yield one group; a
  dropped scan sender clears `scan` and shows a toast.
