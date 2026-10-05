# Changelog

## Unreleased

- App icon: the kHz mark (dark variant) for the window, desktop file and app
  launchers. Re-run the icon lines of the README install block to get it.
  All variants live in `assets/icons/`.

## 0.1.4 — 2026-10-05

- Renamed to khz-player. The binary, desktop file, Wayland app id and MPRIS
  name (`org.mpris.MediaPlayer2.khz-player`) follow the new name; update any
  Hyprland float rule from `^(rmp)$` to `^(khz-player)$`.
- Existing `~/.config/rmp` and `~/.local/share/rmp` are moved to
  `~/.config/khz-player` and `~/.local/share/khz-player` on first start.

## 0.1.3 — 2026-10-05

Audio fixes.

- Seeking in M4A/AAC files lands where the time display says.
- New formats: ALAC (in M4A/MP4), AIFF, MP1 and MP2. Opus, WavPack, APE and
  Musepack files were listed but never played; `.opus`, `.wv`, `.ape` and
  `.mpc` files now leave the library on the next rescan.
- Chained Ogg streams play to the end, and a sample-rate change in the middle
  of a file keeps the right pitch.
- If the audio device disappears or fails to open, rmp retries every 2 s and
  continues at the same position once a device is back. Paused stays paused.
- The output format is picked from what the device supports, and every
  sample format cpal offers works.
- Volume and balance changes ramp over 10 ms instead of stepping, so fast
  drags no longer crackle. Loud signals (e.g. EQ +12 dB) are soft-clipped
  instead of hard-clipped.
- Lower idle CPU with the EQ on: filter state no longer decays into slow
  denormal numbers in silence.
- The next track is opened in the background, so a slow disk no longer
  stutters the current one.
- Buffer underruns are counted and logged at most every 10 s instead of
  one warning each.

## 0.1.2 — 2026-10-04

- The window title shows the current track: `Artist - Title - rmp`.
- New `animation_fps` setting in `config.toml` (10–60, default 30): how often
  the window repaints while playing. Lower values use less CPU, e.g. about
  11 % at 30 fps and 7 % at 20 fps on a Ryzen 7 4700U.
- The playlist follows the playing track again on auto-advance, Next/Prev
  and jumps. It scrolls only when that track is out of view, so a
  double-click on a visible row leaves the list where it is.
- Playlist and library scroll smoothly, without small jumps, and no longer
  show blank space after the last row.
- The library tree lays out only the rows in view, so a big library no
  longer costs CPU every frame.
- A file that failed to load shows muted even when it has tags.

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
