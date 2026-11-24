# Slice 11: Video settings, shortcut, window close, fallback toggle

Part C slice 11 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 11'); constraints: outline-constraints.md next to it.

Boundary: `src/ui/settings.rs`, `src/shortcuts.rs`, `src/app`
Depends on: Slice 3, Slice 10
1. Video on/off, `Alt+V`, window-closed handling, W = 2 (files 4: `src/ui/settings.rs`,
   `src/shortcuts.rs`, `src/app/mod.rs`, `src/app/playback.rs` → 1; +1 shared contract, the
   shortcut table).
   Scope: settings toggle for `cfg.video.enabled` (switching it off while a video plays stops
   it and replays the track through the engine at the same position), `Action::ShowVideo`
   raising the window, `VideoEvent::WindowClosed` stops the track (state Stopped, no advance),
   a status row when libmpv is unavailable.
ΣW = 2.
Must verify by hand under Hyprland: the float rule on app-id `khz-player-video` applies, the
window does not steal focus from the main window on each track (if it does, set mpv
`focus-on=never`, available in mpv 0.41), and `q` in the mpv window closes it and reaches the
app as `WindowClosed`.
