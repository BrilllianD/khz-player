# Scanner deletes tracks and AUTO EQ mappings it should keep

Two paths end in `delete_unseen` / `delete_outside_roots` removing rows of files that still
exist, together with their `eq_auto` rows, which never come back. (1) Nested roots: with roots
`/m` and `/m/nas` and `/m/nas` unmounted, the outer root's prune covers the inner range while
the toast says "kept its tracks" (real R1). (2) After `config.toml` is set aside as `.broken`,
`library_roots` is the default `~/Music` and `delete_outside_roots` runs even when no configured
root walked successfully, so the next start scan drops the real library (real Y18).

- Where: `src/library/scanner.rs:88-107,137-138`, `src/library/db.rs:281-324`
- Done when: a scanner test with roots `[music, music/sub]` scans, empties `music/sub`, rescans,
  and the `sub` tracks and their `eq_auto` rows survive; a scan with `roots = [missing]` after a
  scan with `roots = [real]` keeps the `eq_auto` rows under `real`.
