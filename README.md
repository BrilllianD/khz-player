# khz-player

A Winamp-classic style music player for Linux, written in Rust.

- Main window with time display, scrolling title, spectrum analyzer, volume and balance.
- 10-band equalizer with presets (built-in and your own) and AUTO mode, which
  remembers the preset chosen for each track.
- Several playlists, stored in SQLite; M3U import and export.
- Media library scanned from your music folders, with a search box.
- Gapless playback; MP3 (and MP1/MP2), FLAC, Ogg Vorbis, AAC and ALAC in
  M4A/MP4, WAV and AIFF. Opus, WavPack, APE and Musepack are not supported.
- MPRIS, so media keys and desktop widgets can control it.
- Uses the current Omarchy theme colors and follows theme switches;
  falls back to the classic Winamp palette elsewhere.

## Build and run

Needs Rust 1.98 or newer, plus ALSA development headers (`alsa-lib`).
On PipeWire systems audio goes through `pipewire-alsa`.

```sh
cargo run --release -- [files | folders | playlist.m3u]...
```

Arguments are added to the current playlist and the first one starts playing.
Without arguments khz-player loads the last track; if you quit mid-track, it waits
paused at the same spot.

To install for your user:

```sh
cargo install --path . --root ~/.local --locked
install -Dm644 khz-player.desktop ~/.local/share/applications/khz-player.desktop
for s in 16 24 32; do
  install -Dm644 assets/icons/dark-small/png/khz-icon-dark-small-$s.png ~/.local/share/icons/hicolor/${s}x${s}/apps/khz-player.png
done
for s in 48 64 128 256 512; do
  install -Dm644 assets/icons/dark/png/khz-icon-dark-$s.png ~/.local/share/icons/hicolor/${s}x${s}/apps/khz-player.png
done
install -Dm644 assets/icons/dark-small/khz-icon-dark-small.svg ~/.local/share/icons/hicolor/scalable/apps/khz-player.svg
update-desktop-database ~/.local/share/applications
```

`--root ~/.local` puts the binary in `~/.local/bin`, which is on the PATH of an
Omarchy session; `~/.cargo/bin` is not, so app launchers would not find `khz-player`
there.

## Keyboard

| Key | Action |
| --- | --- |
| `Z` `X` `C` `V` `B` | Previous, play (restarts when playing), pause / resume, stop, next |
| `Space` | Play / pause |
| `←` `→` | Seek 5 s back / forward |
| `↑` `↓`, `+` `-` | Volume up / down |
| `R` | Cycle repeat (off, all, one) |
| `S` | Toggle shuffle |
| `L` | Focus the open-path field (file, folder or `.m3u`) |
| `J` | Jump to a track by name |
| `Enter` | Play the selected track |
| `Del` | Remove selected tracks |
| `Ctrl+A` | Select all |
| `Alt+G` / `Alt+E` / `Alt+L` | Show or hide equalizer / playlist / library |

## Mouse

- Double-click a track to play it; right-click for its menu.
- Drag tracks to reorder them; a selection moves as one block.
- Shift-click and Ctrl-click select ranges and single tracks.
- Click the time display to switch between elapsed and remaining.
- Scroll over the volume bar to change volume; double-click the balance bar to center it.
- Drag the title strip to move the window.
- Drop files or folders onto the window to add them. Wayland drag and drop
  may not work under every compositor; the open-path field (`L`) and the
  library always do.

The playlist footer menus hold the rest: ADD, REM (remove missing files,
duplicates by path or by tags), SEL, MISC (sort, reverse, randomize) and LIST
(new, rename, delete, import and export M3U).

## Files

| Path | Contents |
| --- | --- |
| `~/.config/khz-player/config.toml` | Settings, window state, last track and position |
| `~/.config/khz-player/eq_presets.toml` | Your equalizer presets |
| `~/.local/share/khz-player/library.db` | Library, playlists, per-track AUTO presets |

A pre-rename `~/.config/rmp` or `~/.local/share/rmp` is moved to the new
location automatically on first start.

Useful keys in `config.toml`:

- `library_roots`: folders to scan (defaults to your music folder).
- `font_path`: font file to use. khz-player looks for JetBrainsMono Nerd Font by
  default and uses its icons when found.

## Hyprland

khz-player draws its own title bar and is meant to float. With Omarchy's Lua config,
add to `~/.config/hypr/looknfeel.lua`:

```lua
o.window("^(khz-player)$", { float = true, size = { 560, 760 } })
```

Without the Omarchy helper:

```lua
hl.window_rule({ match = { class = "^(khz-player)$" }, float = true, size = { 560, 760 } })
```

## MPRIS

khz-player registers as `org.mpris.MediaPlayer2.khz-player`.

```sh
playerctl -p khz-player play-pause
busctl --user call org.mpris.MediaPlayer2.khz-player /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.Player PlayPause
busctl --user get-property org.mpris.MediaPlayer2.khz-player /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.Player Metadata
```

## Logging

```sh
RUST_LOG=khz_player=debug khz-player
```

## License

khz-player is free software, licensed under the GNU General Public License,
version 3 or (at your option) any later version. See `LICENSE`.

Third-party crates keep their own licenses: mostly MIT and Apache-2.0, and
MPL-2.0 for symphonia and mpris-server. The fonts egui embeds are under the
OFL-1.1 and the Ubuntu Font Licence.
