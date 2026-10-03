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
