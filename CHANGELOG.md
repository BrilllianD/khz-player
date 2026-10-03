# Changelog

## 0.1.0 — 2026-10-03

First release.

- Winamp-classic main window: time display (click for remaining), scrolling
  title, spectrum analyzer, volume, balance, seek bar, transport buttons.
- Own audio engine on symphonia, rubato and cpal: MP3, FLAC, Ogg Vorbis,
  AAC/M4A and WAV, accurate seeking, gapless playback.
- 10-band equalizer with preamp, built-in and user presets, and AUTO mode
  that remembers a preset per track.
- Several playlists stored in SQLite, M3U import and export, sorting,
  reverse, randomize, removal of missing files and of duplicates by path or
  by tags, drag-reorder of rows, jump-to-track dialog.
- Media library scanned from configured folders, incremental rescans,
  search; repairs CP1251 tags read as Latin-1 and ignores tags lost to
  `?` characters.
- Resumes the last track, paused at the saved position.
- MPRIS (`org.mpris.MediaPlayer2.rmp`): play state, metadata, position,
  seek, volume, shuffle, loop status, quit.
- Follows the current Omarchy theme and recolors on theme switches; classic
  Winamp palette elsewhere. Uses JetBrainsMono Nerd Font icons when found.
- Winamp keyboard shortcuts (Z X C V B and more), desktop entry.
