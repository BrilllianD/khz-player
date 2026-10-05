# A: bug sprint after 0.1.0

Fix the 29 defects found in the post-0.1.0 review (audio engine, app/UI/playlist, library/DB/MPRIS/M3U/packaging). Every item is a related task tagged `bug` (tasks 22 to 50 = A1 to A29).

Order: playlist/persistence/UI day first (remove-current ghost position, dirty split, broken config, save loop, debounce, gapless mislabel, skip on load error, next/prev after playlist delete, MPRIS Play/C toggle, stat per row, drag flag + repaint floor, volume wheel), then release 0.1.1. Then audio day (seek trim, rate change, device loss, formats, codec features), then library/MPRIS/M3U/UI rest.

Each fix gets a unit test where the logic is pure (playlist, m3u, config, tags, decoder trim, CallbackState::fill).

Plan: ~/.claude/plans/review-and-research-what-agile-flurry.md

## Status 2026-10-03

Landed in 0.1.1 (tag v0.1.1): A1 to A12 (tasks 22 to 33), one commit per step, tests 45 -> 60.
A12 volume wheel reads raw MouseWheel events (Line unit = 1 notch), not raw_scroll_delta, which egui 0.36 removed.
A8 needs no app split: delete_playlist hands playing_list to the active list right away.
Manual checks for A1, A3, A4/A5 (inotifywait), A7, A8, A9, A11, A12 are still open; see the source plan's verification list.

Remaining: A13 to A29 (tasks 34 to 50).
Next session: audio pass A13 to A17, then library/MPRIS/M3U A18 to A26, then UI A27 to A29. After that Track B, starting with B1 single instance (fixes A25).

## Status 2026-10-05

0.1.2 (tag v0.1.2, 2026-10-04) shipped UI and perf fixes instead of the audio pass: tasks 88 to 91 (window title, playlist scroll, animation_fps, library tree layout, muted failed row). Tests 60 -> 63. Manual checks 79 to 87 for 0.1.1 are all done.

Next session: audio pass, released as 0.1.3. `src/audio` is unchanged since v0.1.1, so the line hooks in the task bodies still hold. The detailed plan for each step now lives in the task bodies (moved from PLAN_TODO.md). Order:
1. A13 seek trim (34) with E7 decoder tests (74)
2. A17 formats, denormals, xrun counter, prefetch thread (38)
3. A14 rate change and ResetRequired (35)
4. A16 output format, gain ramp, soft clip (37) with E8 CallbackState tests (75)
5. A15 device loss recovery (36)
6. E9 engine gapless tests (76)
7. Release 0.1.3: CHANGELOG in user terms, bump Cargo.toml, `chore: release 0.1.3`, tag v0.1.3 locally, note the release binary size. File `user-manual` tasks for the manual checks in each body.

Before starting, offer the older `user-checks` 12 to 17, especially 15 (gapless by ear), because A17 and A15 touch that path.
Do not run `cargo fmt` on the whole tree: `app.rs` is not rustfmt-clean and a mass reformat buries the real diffs.

After the audio pass: library/MPRIS/M3U A18 to A26 (39 to 47) with E5, E10, E11; then UI A27 to A29 (48 to 50); then Track B starting with B1 (51, also fixes A25).
