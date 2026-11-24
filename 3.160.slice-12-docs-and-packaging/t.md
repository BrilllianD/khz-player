# Slice 12: Docs and packaging

Part C slice 12 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 12'); constraints: outline-constraints.md next to it.

Boundary: `README.md`, `CHANGELOG.md`, `docs/design.md`
Depends on: Slice 7, Slice 11
1. Docs for Opus, download and video, W = 2 (files 3-4: `README.md`, `CHANGELOG.md`,
   `docs/design.md` → 1; +1 shared contract, the documented config keys `[download]` and
   `[video]`). Scope: features, Ctrl+L and Alt+V, yt-dlp config note (user config applies,
   parser flags always passed), runtime deps (`yt-dlp`, `ffmpeg`, `mpv`/libmpv, optional
   mutagen), build deps (`cmake`, `libmpv-dev`), the Hyprland float rule for
   `khz-player-video`, the tarball's `video` status from slice 9, an Unreleased changelog
   section. The end-to-end check of the prior plan's Verification runs here, by hand.
ΣW = 2.
