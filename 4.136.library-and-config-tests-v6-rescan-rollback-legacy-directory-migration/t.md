# Library and config tests: v6 rescan, rollback, legacy directory migration

No test checks that a scan re-reads the `mtime = -1` rows left by v6, that a failing
`migrate_v6` rolls back with the original error, or that pre-v4 playlists survive v4 (real
Y20). `migrate_legacy_dirs()` reads the real `ProjectDirs` and has no test (real Y21).

- Where: `src/library/db.rs:116`, `src/library/scanner.rs:114`, `src/config.rs:169`
- Done when: a v5 database with one wav migrates, scans and reports `updated == 1`; a forced
  `migrate_v6` failure leaves `user_version == 5` with the error text; a pure
  `migrate_dirs(pairs)` is tested for moved, both-exist and missing-parent.
