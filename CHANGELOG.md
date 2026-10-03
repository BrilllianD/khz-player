# Changelog

## 0.1.1 — 2026-10-03

Fixes.

- Removing the playing track keeps it playing and then continues with the
  track that followed it, instead of jumping to the top of the list.
- A file that cannot be opened is skipped instead of stopping playback; it
  shows muted in the playlist. A list where nothing plays stops after one pass.
- Stopping or picking a track right at the end of the previous one no longer
  moves the playlist marker past the track you chose.
- Next and previous work again after deleting the playlist that was playing.
- `C` resumes a paused track (Winamp behaviour); MPRIS Play no longer
  restarts a track that is already playing.
- A `config.toml` or `eq_presets.toml` that rmp cannot read is kept as
  `*.broken` and reported, instead of being overwritten with defaults.
  Out-of-range values in the config are clamped.
- Changing tracks no longer rewrites the whole playlist in the database.
- Settings are saved once you stop changing them (at most every 10 s during
  a long drag), and a failed playlist save no longer retries every second.
- One mouse-wheel notch over VOL changes the volume by 2 %.
- No more busy repainting after a drag released outside the window, and
  the playlist no longer checks every visible file on disk each frame.

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
