# Plan: `rmp` — Winamp-classic style music player in Rust (egui/eframe)

## Context

User wants a desktop music player with playlists, a 10-band equalizer, and a compact Winamp classic / qmmp look with a dark theme that follows the Omarchy desktop theme. Target dir `/home/bronnikov/Projects/rmp` is empty (greenfield, not a git repo). Environment: Arch/Omarchy, Hyprland (Wayland), PipeWire 1.6.8 (pipewire-alsa + pipewire-pulse), Rust 1.98.1, ~1218 mp3/flac in `~/Music`, Nerd Fonts installed at `/usr/share/fonts/TTF/{JetBrainsMono,CaskaydiaMono}NerdFont-Regular.ttf`. Current Omarchy theme: osaka-jade, colors at `~/.local/state/omarchy/current/theme/colors.toml` (symlinked dir swapped on theme change).

Decisions made with user:
- Stack: Rust + egui/eframe (glow renderer).
- Theme: auto-read Omarchy `colors.toml` with live reload; built-in Winamp-classic dark palette fallback.
- Playlists: multiple named playlists (tabs), M3U/M3U8 import/export.
- Extras in v1: spectrum visualizer, MPRIS D-Bus, library scan of `~/Music`, 10-band EQ with Winamp presets + saved user presets.
- Library/playlist persistence: SQLite via `rusqlite` (bundled).

## Dependencies (`Cargo.toml`)

```toml
[package]
name = "rmp"
edition = "2024"
rust-version = "1.98"

[dependencies]
eframe = { version = "0.36.2", default-features = false, features = ["glow", "wayland", "x11", "default_fonts"] }
egui   = "0.36.2"
cpal      = "0.18.2"
symphonia = { version = "0.6.1", default-features = false, features = ["mp3", "flac", "vorbis", "ogg", "aac", "isomp4", "wav", "pcm"] }
rubato    = "5.0.1"
realfft   = "3.5.0"
rtrb      = "0.4"
lofty     = "0.25.4"
rusqlite  = { version = "0.40.2", features = ["bundled"] }
walkdir   = "2.5"
toml      = "1.1"
serde     = { version = "1", features = ["derive"] }
directories = "6.0"
notify    = "8.2"
crossbeam-channel = "0.5"
mpris-server = "0.10"      # pin "0.9" if 0.10 fails to build
futures-lite = "2"
async-channel = "2"
rand = "0.9"
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[profile.dev.package."*"]
opt-level = 2   # symphonia/rustfft/sqlite too slow at O0, keeps dev audio glitch-free
```

Key choices: glow over wgpu (compile time, binary size); own audio engine (symphonia + cpal + rtrb ring) instead of rodio (need accurate seek, post-EQ spectrum tap, gapless prefetch); `mpris-server` over souvlaki (full spec: Position, Seeked, LoopStatus, Shuffle); playlists stored in SQLite, M3U only import/export; hand-rolled M3U parser (~40 lines); library search in memory (1200 tracks). No `rfd` in v1 (optional in P8).

## Module layout

```
src/
  main.rs            tracing init, load config, spawn engine/mpris/theme-watcher, eframe::run_native
  app.rs             App (eframe::App): owns state, drains event channels, dispatches to ui/*
  config.rs          Config (serde) load/save ~/.config/rmp/config.toml via `directories`
  theme.rs           Theme struct, lenient Omarchy colors.toml parser, Winamp fallback, apply to egui Style
  theme_watch.rs     notify watcher on ~/.local/state/omarchy/current/ (parent dir, not symlink target), 250ms debounce
  fonts.rs           system Nerd Font path probe -> FontDefinitions; fallback egui bundled Hack
  audio/mod.rs       Command/Event enums, PlayerState, AudioHandle
  audio/shared.rs    Arc<Shared>: atomics (position_frames, flush, DspParams, generation)
  audio/engine.rs    decode thread: state machine, decoder lifecycle, prefetch next, seek, feed ring
  audio/decoder.rs   symphonia 0.6 wrapper: open, next_frames (interleaved f32), seek
  audio/resample.rs  rubato Fft wrapper, bypass when track rate == device rate
  audio/output.rs    cpal stream at device default rate; realtime callback: ring -> EQ -> vol/bal -> spectrum tap
  audio/dsp.rs       Biquad (RBJ peaking), Equalizer (10 bands x 2 ch, preamp, smoothing), volume/balance
  audio/spectrum.rs  UI-side FFT: Hann, realfft 2048, 20 log bands, dB, fall-off + peak hold
  eq_presets.rs      Winamp preset table, user presets TOML load/save
  library/mod.rs     Track struct, LibraryHandle
  library/db.rs      rusqlite open/migrate, tracks + playlists CRUD
  library/scanner.rs background thread: walkdir, mtime/size compare, lofty, batched inserts, progress
  library/tags.rs    lofty -> Track (title fallback = file stem)
  playlist.rs        in-memory Playlist (Vec<Track>, cursor, shuffle order, repeat), next/prev
  m3u.rs             parse/write M3U/M3U8 (#EXTM3U/#EXTINF, relative paths)
  mpris.rs           thread: mpris-server Player, zbus::block_on, Command sender + async_channel updates
  shortcuts.rs       key map -> Action
  ui/mod.rs          layout root: main / eq / playlist stack, library SidePanel::right
  ui/main_panel.rs   title strip, digits, marquee, spectrum, kbps/kHz, seek, vol/bal, transport, toggles
  ui/widgets.rs      custom painters: digits, marquee, spectrum, led_toggle, winamp_button
  ui/eq_panel.rs     ON/AUTO, preamp + 10 vertical sliders, presets ComboBox, save/delete preset
  ui/playlist_panel.rs tabs, list, ADD/REM/SEL/MISC/LIST menus, drag-drop, dbl-click
  ui/library_panel.rs search, rescan + progress, Artist > Album > Track tree, add to playlist
  ui/jump_dialog.rs  J: filter current playlist, Enter plays
```

## Threading

| Thread | Role | Communication |
|---|---|---|
| UI (eframe) | drains `Receiver<Event>`, `ScanEvent`, `ThemeEvent`; sends `Command`, `MprisUpdate` | crossbeam-channel |
| audio-engine | `recv_timeout(2-5ms)` loop, decode + resample, push to `rtrb::Producer<f32>` | sends `Event` + `ctx.request_repaint()` |
| cpal callback | `rtrb::Consumer` -> DSP -> out; spectrum tap `rtrb::Producer` (cap 8192, drop if full) | reads `Arc<Shared>` atomics only; no locks/alloc/log |
| scanner | own rusqlite Connection, WAL, 200-row transactions | `ScanEvent::{Progress, Done(Vec<Track>), Error}` |
| mpris | `zbus::block_on(zip(player.run(), update_loop))` | pushes `Command`; `async_channel::Receiver<MprisUpdate>` |
| theme-watch | notify callback, debounce | `ThemeEvent::Changed` + repaint |

Ring buffer: stereo interleaved f32 at output rate, ~300 ms capacity. Engine upmixes mono / downmixes >2ch before push.

## Audio engine

```rust
pub enum Command { Load{path, play}, Play, Pause, TogglePlay, Stop, Seek(Duration), SeekRel(i64),
                   PrefetchNext(Option<PathBuf>), Shutdown }
pub enum Event { Loaded(TrackInfo), StateChanged(PlayerState), TrackEnded, Error(String) }
```
- Position read by UI from `position_frames / out_rate` each frame (no events).
- States: Stopped / Playing / Paused. Pause = stop pushing; callback outputs silence when ring empty, stream never paused.
- Seek: `reader.seek(SeekMode::Accurate, SeekTo::Time{..})`, `decoder.reset()`, resampler reset, set `flush` flag, wait (<=100 ms) for callback to drain ring, set `position_frames`, discard frames until `required_ts`.
- Gapless: `PrefetchNext` opens next decoder in advance; on EOF swap decoders without flush; send `TrackEnded` + `Loaded(next)`.
- symphonia 0.6 API: `probe(&hint, mss, fmt_opts, meta_opts)`, `default_track(TrackType::Audio)`, `codec_params.audio()`, `make_audio_decoder`, `next_packet -> Result<Option<Packet>>`, `decode -> GenericAudioBufferRef`, `copy_to_vec_interleaved`. `enable_gapless: true`.
- cpal 0.18 API: `default_output_config()` (expect 48000), force 2 ch, `BufferSize::Fixed(1024)` with Default fallback, request f32 else iterate `supported_output_configs`; `build_output_stream(config, cb, err, None)`, `stream.start()`.
- rubato 5: `Fft::<f32>::new(in_rate, out_rate, 1024, 2, FixedSync::Input)` with interleaved adapters; accumulate 1024-frame input chunks; reset on seek. Fallback: reopen stream at track rate.

### EQ (`audio/dsp.rs`)
Bands Hz `[60,170,310,600,1000,3000,6000,12000,14000,16000]`, ±12 dB, Q = 1.2, clamp f0 to `min(f, out_rate*0.45)`. RBJ peaking:
```
A = 10^(g/40); w0 = 2πf0/fs; α = sin(w0)/(2Q)
b0 = 1+αA   b1 = -2cos(w0)   b2 = 1-αA
a0 = 1+α/A  a1 = -2cos(w0)   a2 = 1-α/A   (normalize by a0)
```
Transposed DF2 per channel. Smoothing: every 64 frames `cur += (target-cur)*0.15`, recompute coeffs when `|Δ|>0.01`. Preamp linear, smoothed. Volume `v²`; balance `l = vol*min(1,1-bal)`, `r = vol*min(1,1+bal)`.

Presets: XMMS/Audacious Winamp table (±20 scale) × 0.6 → ±12: Classical, Club, Dance, Full Bass, Full Bass & Treble, Full Treble, Laptop Speakers/Headphones, Large Hall, Live, Party, Pop, Reggae, Rock, Ska, Soft, Soft Rock, Techno (values as in agent table, e.g. Rock `8 4.8 -5.6 -8 -3.2 4 8.8 11.2 11.2 11.2`). User presets → `~/.config/rmp/eq_presets.toml` `[[preset]] name, preamp, bands=[..10]`.

### Spectrum (`audio/spectrum.rs`, UI thread)
Drain tap into rolling 2048-sample buffer; Hann; realfft; 20 log bands 50 Hz–16 kHz (`f_k = 50*(320)^(k/20)`), max magnitude per band; dB `[-60,0] → [0,1]`; divide by `vol_lin` (floor 0.05) before dB; fall 0.08/frame, peak hold 15 frames then 0.02/frame; `request_repaint_after(1/animation_fps)` while playing (default 30).

## UI layout (logical points, Winamp 275×116 ≈ ×2)

```
Main 560×232 (always)
  title strip 560×20 "RMP" + [_][x]; drag -> ViewportCommand::StartDrag, close -> Close
  [digits 180×60 "-02:34"] [marquee 340×20 title]
                           [spectrum 340×60, 20 bars]
  [kbps][kHz] [mono/stereo LED]  vol 220  bal 100
  [seek 540×12]
  [|<][>][||][■][>|] [open]   [shuffle][repeat] [EQ][PL]
EQ 560×200 (toggle Alt+G / EQ): [ON][AUTO] preamp vslider | 10 vsliders | Presets ComboBox
Playlist 560×fill min 200 (toggle Alt+E / PL): tabs [Default][Rock][+]; ScrollArea rows
  "1. Artist - Title   3:45"; [ADD v][REM v][SEL v][MISC v][LIST v]  total
Library SidePanel::right 420 (Alt+L): search, rescan + ProgressBar, Artist > Album > Track tree
```
Viewport: `with_inner_size([560,760]).with_min_inner_size([560,232]).with_decorations(false).with_app_id("rmp")`. Resize on panel toggle via `ViewportCommand::InnerSize` (floating only). Layout fluid so tiled also works. Document Hyprland float rule for user.

Stock egui: Slider (seek/vol/bal/`Slider::vertical()` EQ), Button, SelectableLabel, ComboBox, ScrollArea, CollapsingHeader, TextEdit, menu_button, ProgressBar. Custom paint via `allocate_painter`: digits (monospace 40pt), marquee (clipped galley, 40 px/s), spectrum bars (gradient lo→hi, 2px peak line), LED toggles. Style: `Visuals::dark()` + overrides, `CornerRadius::ZERO`, tight spacing → flat Winamp look.

Fonts: try `config.font_path`, JetBrainsMono NF, CaskaydiaMono NF, then `fc-match -f '%{file}' monospace`; insert as first in Monospace and Proportional families; `set_fonts` once at startup.

### Theme
```rust
pub struct Theme { bg, bg_dark, bg_darker, panel, frame, text, text_dim, text_bright, digits,
                   accent, selection, muted, spectrum_lo, spectrum_hi, peak, warn: Color32, dark: bool }
```
Omarchy mapping: bg=background, bg_dark=dark_background, bg_darker=darker_background, panel=lighter_background, frame=muted, text=foreground, text_dim=dark_foreground, text_bright=bright_foreground, digits/accent=accent, selection=selection, spectrum_lo=green, spectrum_hi=bright_yellow, peak=light_foreground, warn=red. Parse as `toml::Table`, accept only `#rrggbb[aa]` strings, skip others (e.g. `hyprland_active_border = "rgba(...) 45deg"`), per-role fallback. Winamp fallback: bg `#000000`, panel `#1e2a3a`, frame `#3b4b6b`, text/digits/accent `#00ff00`, selection `#0000c6`, muted `#5a6a8a`, spectrum `#00ff00`→`#ffff00`, peak `#c0c0c0`.

Watch parent `~/.local/state/omarchy/current/` NonRecursive (symlink swapped atomically); re-resolve path on each reload.

### Shortcuts (only when `!ctx.wants_keyboard_input()`)
Z prev, X play, C pause, V stop, B next, Space toggle, L focus open-path field, J jump dialog, Left/Right ±5 s, Up/Down vol ±2%, R repeat cycle Off/All/One, S shuffle, Alt+G EQ, Alt+E playlist, Alt+L library, Delete remove selected, Enter play selected, Ctrl+A select all.

## Persistence

SQLite `~/.local/share/rmp/library.db`, WAL, `user_version` migrations:
```sql
CREATE TABLE tracks (path TEXT PRIMARY KEY, mtime INTEGER NOT NULL, size INTEGER NOT NULL,
  title TEXT, artist TEXT, album TEXT, album_artist TEXT, track_no INTEGER, disc_no INTEGER,
  year INTEGER, genre TEXT, duration_ms INTEGER, bitrate INTEGER, sample_rate INTEGER,
  channels INTEGER, seen INTEGER NOT NULL DEFAULT 1);
CREATE INDEX idx_tracks_artist_album ON tracks(artist, album, disc_no, track_no);
CREATE TABLE playlists (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, position INTEGER NOT NULL);
CREATE TABLE playlist_items (playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
  position INTEGER NOT NULL, path TEXT NOT NULL, PRIMARY KEY (playlist_id, position));
CREATE TABLE eq_auto (path TEXT PRIMARY KEY, preset TEXT NOT NULL);
```
Scanner: clear `seen`, set per visit, delete `seen=0`. Playlist item metadata resolved from `tracks` by path, else lofty on demand.

`~/.config/rmp/config.toml`: version, library_roots, font_path, volume, balance, shuffle, repeat (off|all|one), last_playlist, last_track_index, show_eq/show_playlist/show_library, time_remaining, animation_fps (10-60, default 30), `[eq] enabled, auto, preamp, bands[10], preset`, `[window] x y w h`. Save debounced 1 s + on exit.

## Phases (each ends runnable)

1. **Skeleton + theme + fonts**: Cargo.toml, main/app/config/theme/theme_watch/fonts, static ui/mod.rs with all panels and dummy values, title strip drag/close.
2. **Audio one file**: audio/{shared,decoder,resample,output,engine,mod}; open-path TextEdit + L; play/pause/stop/seek; digits; kbps/kHz.
3. **Playlist**: playlist.rs, m3u.rs, db.rs (playlist tables), playlist_panel; prev/next/shuffle/repeat; gapless prefetch; auto-advance; drag-drop (`i.raw.dropped_files`); tabs CRUD; M3U import/export; persist.
4. **EQ**: dsp.rs, eq_presets.rs, eq_panel, DspParams atomics.
5. **Spectrum + marquee**: spectrum.rs, callback tap, widgets.
6. **Library + SQLite scan**: scanner/tags/db tracks table, library_panel, progress, incremental rescan, search, add to playlist.
7. **MPRIS**: mpris.rs.
8. **Polish**: remaining shortcuts, J dialog, window/volume persistence, EQ AUTO, time-remaining toggle, error toasts, optional `rfd`, clippy `-D warnings`, release build.

## Verification

- P1: `cargo run` opens window on Hyprland with Nerd Font and osaka-jade colors; switch theme via Omarchy theme menu → recolors in ~0.3 s without restart; temporarily break `colors.toml` → green Winamp fallback. `RUST_LOG=rmp=debug`.
- P2: `cargo run -- "$HOME/Music/Nero - Satisfy.mp3"` plays; seek lands within ~0.1 s, no click; pause/resume exact; flac + m4a play; pitch matches `mpv` (resampler 44.1k→48k correct); `pw-top` shows stream.
- P3: drop a dir from file manager → tracks appear (if Wayland DnD fails in winit, fall back to typed path / library add; note it); dbl-click plays; B/Z navigate; two album tracks back-to-back without gap; export M3U, restart, playlist restored; `mpv --playlist=x.m3u` reads it.
- P4: toggle ON audible; fast slider drags no clicks; Full Bass obviously bassy; save user preset, restart, present; preamp -12 quieter.
- P5: bars move, fall smoothly, peaks hold; stop → decay to zero; CPU < 5% while playing.
- P6: first scan of ~1218 files finishes in seconds with progress; `sqlite3 ~/.local/share/rmp/library.db 'select count(*) from tracks'`; touch one mp3 → rescan updates only it; delete file → row gone; search filters live.
- P7: `busctl --user list | grep mpris` shows `org.mpris.MediaPlayer2.rmp`; `busctl --user call ... PlayPause`; install `playerctl` (`sudo pacman -S playerctl`) → `playerctl -p rmp status|metadata|next`; waybar mpris + media keys work.
- P8: `cargo clippy -- -D warnings`, `cargo build --release`.

## Risks / gotchas

- Wayland drag-and-drop in winit may be unsupported on Hyprland; don't block on it.
- Hyprland tiling ignores `InnerSize`; document float rule `^(rmp)$`.
- cpal 0.18 and symphonia 0.6 have breaking API changes vs older examples; verify against `cargo doc`.
- Realtime callback: no Mutex/alloc/logging; flush must happen in callback, not engine.
- `mpris-server` Player is `!Send` → dedicated thread; 0.10 docs.rs build failed, pin 0.9 if needed.
- rusqlite bundled compiles SQLite C once (~30–60 s). First dev build several minutes.
- Winamp preset tables published in ±20 scale → ×0.6.
- Theme `theme` is a symlink swapped on switch → watch parent dir, re-resolve.
