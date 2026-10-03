# A: bug sprint after 0.1.0

Fix the 29 defects found in the post-0.1.0 review (audio engine, app/UI/playlist, library/DB/MPRIS/M3U/packaging). Every item is a related task tagged `bug`.

Order: playlist/persistence/UI day first (remove-current ghost position, dirty split, broken config, save loop, debounce, gapless mislabel, skip on load error, next/prev after playlist delete, MPRIS Play/C toggle, stat per row, drag flag + repaint floor, volume wheel), then release 0.1.1. Then audio day (seek trim, rate change, device loss, formats, codec features), then library/MPRIS/M3U/UI rest.

Each fix gets a unit test where the logic is pure (playlist, m3u, config, tags, decoder trim, CallbackState::fill).

Plan: ~/.claude/plans/review-and-research-what-agile-flurry.md
