# Slice 2: Shared background-job contract (audit 142, part 1)

Part C slice 2 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 2'); constraints: outline-constraints.md next to it.

Boundary: `src/job.rs` and its two users in `src/library` and `src/app`
Depends on: none
1. `job::Job` and scanner/adder migration, W = 4 (files 7: `src/job.rs` new, `src/lib.rs`,
   `src/library/scanner.rs`, `src/library/adder.rs`, `src/app/mod.rs`, `src/app/playlists.rs`,
   `src/app/events.rs` → 2; +2 new interface `Job`/`Sink`/`Cancel`/`Terminal`). Scope: the
   contract in the constraints file, scanner and adder on it, `Poll::Lost` handled as an error,
   tests for terminal, lost and cancel. Produces `job::spawn`.
ΣW = 4. Produces: `job::{spawn, Job, Sink, Cancel, Poll, Terminal}`.
