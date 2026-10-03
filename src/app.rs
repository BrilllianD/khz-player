use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use rusqlite::Connection;

use crate::audio::spectrum::Spectrum;
use crate::audio::{AudioHandle, Command, Event, PlayerState, TrackInfo};
use crate::config::Config;
use crate::eq_presets::{self, Preset};
use crate::library::scanner::{self, ScanEvent};
use crate::library::{Track, db, tags};
use crate::mpris::{self, Mpris, MprisAction, MprisUpdate};
use crate::playlist::Playlist;
use crate::shortcuts::{self, Action};
use crate::theme::Theme;
use crate::theme_watch::{self, ThemeWatcher};
use crate::ui;
use crate::{fonts, m3u};

const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);
/// Save anyway after this long of continuous changes (a long slider drag).
const SAVE_CAP: Duration = Duration::from_secs(10);
const TOAST_TIME: Duration = Duration::from_secs(5);

/// Text prompt shown as a modal (no native file dialogs).
#[derive(Debug, Clone, PartialEq)]
pub enum PromptKind {
    AddPath,
    ImportM3u,
    ExportM3u,
    NewPlaylist,
    RenamePlaylist(usize),
    SavePreset,
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub kind: PromptKind,
    pub text: String,
    pub focus: bool,
}

#[derive(Debug, Default)]
pub struct JumpState {
    pub query: String,
    pub cursor: usize,
    pub focus: bool,
}

pub struct App {
    ctx: egui::Context,
    pub cfg: Config,
    /// Last and first unsaved change to the config or a playlist.
    cfg_changed: Option<Instant>,
    cfg_first_change: Option<Instant>,
    /// A playlist save failed and the user was told; reset by a good save.
    save_failed: bool,
    pub theme: Theme,
    theme_watch: Option<ThemeWatcher>,
    pub nerd_font: bool,

    pub audio: AudioHandle,
    pub spectrum: Spectrum,
    pub state: PlayerState,
    /// Decoder info of the loaded track.
    pub now: Option<TrackInfo>,
    /// Tag info of the loaded track (for the marquee).
    pub now_track: Option<Track>,
    track_counter: u64,
    /// While dragging the seek bar: fraction 0..1.
    pub seek_drag: Option<f32>,
    pub marquee_start: Instant,

    db: Option<Connection>,
    pub playlists: Vec<Playlist>,
    /// Playlist shown in the playlist panel.
    pub active: usize,
    /// Playlist the playing track belongs to.
    pub playing_list: usize,
    /// Files that failed to load this session (shown muted in the playlist).
    pub failed: HashSet<PathBuf>,
    /// Tracks skipped in a row because they failed to load.
    load_failures: usize,

    pub library: Vec<Track>,
    pub library_gen: u64,
    scan: Option<Receiver<ScanEvent>>,
    pub scan_progress: Option<(usize, usize)>,
    pub library_search: String,
    pub library_cache: ui::library_panel::Cache,

    mpris: Mpris,
    last_mpris_pos: Instant,

    pub builtin_presets: Vec<Preset>,
    pub user_presets: Vec<Preset>,

    pub open_path: String,
    pub focus_open: bool,
    pub prompt: Option<Prompt>,
    pub jump: Option<JumpState>,
    pub toasts: Vec<(String, Instant)>,
    /// Scroll the playlist to this row on the next frame.
    pub scroll_to: Option<usize>,
    last_frame: Instant,
}

pub fn window_size(cfg: &Config) -> egui::Vec2 {
    let w = ui::MAIN_W + if cfg.show_library { ui::LIBRARY_W } else { 0.0 };
    let mut h = ui::MAIN_H;
    if cfg.show_eq {
        h += ui::EQ_H;
    }
    if cfg.show_playlist {
        h += cfg.window.h.map_or(ui::PLAYLIST_H, |saved| {
            (saved - ui::MAIN_H - if cfg.show_eq { ui::EQ_H } else { 0.0 }).max(ui::PLAYLIST_MIN_H)
        });
    }
    egui::vec2(w, h)
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Config,
        args: Vec<PathBuf>,
        cfg_error: Option<String>,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        let nerd_font = fonts::install(&ctx, cfg.font_path.as_deref());
        let theme = Theme::load();
        theme.apply(&ctx);
        let theme_watch = theme_watch::start(ctx.clone());

        let audio = AudioHandle::start(cfg.volume, cfg.balance, ctx.clone());
        let spectrum = Spectrum::new(audio.shared.out_rate());

        let db = match db::open(&db::default_path()) {
            Ok(c) => Some(c),
            Err(e) => {
                tracing::error!("database: {e}");
                None
            }
        };
        let library = db
            .as_ref()
            .and_then(|c| db::load_tracks(c).ok())
            .unwrap_or_default();
        let mut playlists = db
            .as_ref()
            .and_then(|c| db::load_playlists(c).ok())
            .unwrap_or_default();
        // Entries outside the library have no cached tags; read them now.
        for t in playlists.iter_mut().flat_map(|p| p.tracks.iter_mut()) {
            if t.mtime == 0 && t.path.exists() && !library.is_empty() {
                *t = tags::read(&t.path);
            }
        }
        if playlists.is_empty() {
            let mut p = Playlist::new("Default");
            p.dirty = true;
            playlists.push(p);
        }
        let active = cfg
            .last_playlist
            .as_ref()
            .and_then(|n| playlists.iter().position(|p| &p.name == n))
            .unwrap_or(0);
        if let Some(i) = cfg.last_track_index
            && i < playlists[active].len()
        {
            playlists[active].current = Some(i);
        }

        let mpris = mpris::start(ctx.clone());
        let (user_presets, presets_error) = eq_presets::load_user();

        let mut app = Self {
            ctx,
            builtin_presets: eq_presets::builtin(),
            user_presets,
            cfg,
            cfg_changed: None,
            cfg_first_change: None,
            save_failed: false,
            theme,
            theme_watch,
            nerd_font,
            audio,
            spectrum,
            state: PlayerState::Stopped,
            now: None,
            now_track: None,
            track_counter: 0,
            seek_drag: None,
            marquee_start: Instant::now(),
            db,
            playlists,
            active,
            playing_list: active,
            failed: HashSet::new(),
            load_failures: 0,
            library,
            library_gen: 0,
            scan: None,
            scan_progress: None,
            library_search: String::new(),
            library_cache: Default::default(),
            mpris,
            last_mpris_pos: Instant::now(),
            open_path: String::new(),
            focus_open: false,
            prompt: None,
            jump: None,
            toasts: Vec::new(),
            scroll_to: None,
            last_frame: Instant::now(),
        };
        app.push_eq();
        app.mpris.send(MprisUpdate::Volume(app.cfg.volume as f64));
        app.mpris.send(MprisUpdate::Shuffle(app.cfg.shuffle));
        app.mpris.send(MprisUpdate::Repeat(app.cfg.repeat));
        for e in cfg_error.into_iter().chain(presets_error) {
            app.toast(e);
        }
        if let Some(e) = app.audio.init_error.clone() {
            app.toast(format!("Audio output unavailable: {e}"));
        }

        if !args.is_empty() {
            let before = app.playlists[app.active].len();
            for a in &args {
                app.add_path(a);
            }
            if app.playlists[app.active].len() > before {
                app.play_index(app.active, before);
            }
        } else if let Some(t) = app.playlists[active].current_track() {
            // Restore the last track without starting playback; paused at the
            // old position if rmp quit mid-track.
            let path = t.path.clone();
            app.audio.send(match app.cfg.last_position_ms {
                Some(ms) => Command::Resume { path, at: Duration::from_millis(ms) },
                None => Command::Load { path, play: false },
            });
        }
        if app.library.is_empty() {
            app.rescan();
        }
        app
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        tracing::warn!("{msg}");
        self.toasts.push((msg, Instant::now()));
    }

    pub fn mark_cfg(&mut self) {
        let now = Instant::now();
        self.cfg_first_change.get_or_insert(now);
        self.cfg_changed = Some(now);
    }

    // ---------------------------------------------------------------- playback

    pub fn play_index(&mut self, list: usize, index: usize) {
        let Some(pl) = self.playlists.get_mut(list) else {
            return;
        };
        if index >= pl.len() {
            return;
        }
        pl.set_current(index);
        let path = pl.tracks[index].path.clone();
        self.playing_list = list;
        self.audio.send(Command::Load { path, play: true });
    }

    fn playing(&mut self) -> &mut Playlist {
        if self.playing_list >= self.playlists.len() {
            self.playing_list = self.active.min(self.playlists.len() - 1);
        }
        &mut self.playlists[self.playing_list]
    }

    pub fn next(&mut self) {
        let (repeat, shuffle) = (self.cfg.repeat, self.cfg.shuffle);
        if let Some(i) = self.playing().next(repeat, shuffle, false) {
            self.play_index(self.playing_list, i);
        }
    }

    pub fn prev(&mut self) {
        // Winamp: restart the track if more than a few seconds in.
        if self.state == PlayerState::Playing && self.audio.position() > Duration::from_secs(5)
        {
            self.audio.send(Command::Seek(Duration::ZERO));
            return;
        }
        let (repeat, shuffle) = (self.cfg.repeat, self.cfg.shuffle);
        if let Some(i) = self.playing().prev(repeat, shuffle) {
            self.play_index(self.playing_list, i);
        }
    }

    pub fn play(&mut self) {
        if self.state == PlayerState::Stopped && self.now.is_none() {
            // Nothing loaded: start the selected or first track of the visible list.
            let pl = &self.playlists[self.active];
            if pl.is_empty() {
                return;
            }
            let idx = pl
                .selected
                .first()
                .copied()
                .or(pl.current)
                .unwrap_or(0);
            self.play_index(self.active, idx);
        } else {
            self.audio.send(Command::Play);
        }
    }

    /// Re-announces the following track to the engine for gapless playback.
    pub fn refresh_prefetch(&mut self) {
        if self.now.is_none() {
            return;
        }
        let (repeat, shuffle) = (self.cfg.repeat, self.cfg.shuffle);
        let pl = self.playing();
        let next = pl
            .peek_next(repeat, shuffle, true)
            .and_then(|i| pl.tracks.get(i))
            .map(|t| t.path.clone());
        self.audio.send(Command::PrefetchNext(next));
    }

    pub fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.cfg.volume = v;
        self.audio.shared.volume.store(v);
        self.mpris.send(MprisUpdate::Volume(v as f64));
        self.mark_cfg();
    }

    pub fn set_balance(&mut self, b: f32) {
        let b = if b.abs() < 0.04 { 0.0 } else { b.clamp(-1.0, 1.0) };
        self.cfg.balance = b;
        self.audio.shared.balance.store(b);
        self.mark_cfg();
    }

    pub fn set_shuffle(&mut self, on: bool) {
        self.cfg.shuffle = on;
        self.mpris.send(MprisUpdate::Shuffle(on));
        self.mark_cfg();
        self.refresh_prefetch();
    }

    pub fn set_repeat(&mut self, r: crate::playlist::Repeat) {
        self.cfg.repeat = r;
        self.mpris.send(MprisUpdate::Repeat(r));
        self.mark_cfg();
        self.refresh_prefetch();
    }

    pub fn duration(&self) -> Option<Duration> {
        self.now
            .as_ref()
            .and_then(|n| n.duration)
            .or_else(|| self.now_track.as_ref().and_then(|t| t.duration()))
    }

    // ---------------------------------------------------------------- EQ

    pub fn push_eq(&self) {
        let e = &self.cfg.eq;
        self.audio.shared.set_eq(&crate::audio::dsp::EqParams {
            enabled: e.enabled,
            preamp_db: e.preamp,
            bands_db: e.bands,
        });
    }

    pub fn apply_preset(&mut self, p: &Preset) {
        self.cfg.eq.bands = p.bands;
        self.cfg.eq.preamp = p.preamp;
        self.cfg.eq.preset = p.name.clone();
        self.push_eq();
        self.mark_cfg();
        if self.cfg.eq.auto
            && let (Some(db), Some(now)) = (&self.db, &self.now)
            && let Err(e) = db::eq_auto_set(db, &now.path, Some(&p.name))
        {
            tracing::warn!("eq auto: {e}");
        }
    }

    pub fn find_preset(&self, name: &str) -> Option<Preset> {
        self.user_presets
            .iter()
            .chain(&self.builtin_presets)
            .find(|p| p.name == name)
            .cloned()
    }

    pub fn save_user_preset(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let p = Preset {
            name: name.clone(),
            preamp: self.cfg.eq.preamp,
            bands: self.cfg.eq.bands,
        };
        match self.user_presets.iter_mut().find(|u| u.name == name) {
            Some(u) => *u = p,
            None => self.user_presets.push(p),
        }
        self.cfg.eq.preset = name;
        self.mark_cfg();
        if let Err(e) = eq_presets::save_user(&self.user_presets) {
            self.toast(format!("Cannot save presets: {e}"));
        }
    }

    pub fn delete_user_preset(&mut self, name: &str) {
        self.user_presets.retain(|p| p.name != name);
        if let Err(e) = eq_presets::save_user(&self.user_presets) {
            self.toast(format!("Cannot save presets: {e}"));
        }
    }

    fn auto_eq_for(&mut self, path: &Path) {
        if !self.cfg.eq.auto {
            return;
        }
        let name = self
            .db
            .as_ref()
            .and_then(|db| db::eq_auto_get(db, path).ok().flatten());
        if let Some(p) = name.and_then(|n| self.find_preset(&n)) {
            self.cfg.eq.bands = p.bands;
            self.cfg.eq.preamp = p.preamp;
            self.cfg.eq.preset = p.name;
            self.push_eq();
        }
    }

    // ---------------------------------------------------------------- playlists

    /// Track metadata from the library, else read from the file.
    pub fn track_for(&self, path: &Path) -> Track {
        self.db
            .as_ref()
            .and_then(|db| db::get_track(db, path).ok().flatten())
            .unwrap_or_else(|| tags::read(path))
    }

    /// Adds a file, directory (recursively) or playlist file to the active playlist.
    pub fn add_path(&mut self, path: &Path) {
        let path = expand_tilde(path);
        if path.is_dir() {
            let mut files: Vec<PathBuf> = walkdir::WalkDir::new(&path)
                .follow_links(true)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file() && tags::is_audio(e.path()))
                .map(|e| e.into_path())
                .collect();
            files.sort();
            let tracks: Vec<Track> = files.iter().map(|p| self.track_for(p)).collect();
            self.playlists[self.active].add(tracks);
        } else if is_playlist_file(&path) {
            self.import_m3u_into(&path, self.active);
        } else if path.is_file() {
            let t = self.track_for(&path);
            self.playlists[self.active].add([t]);
        } else {
            self.toast(format!("Not found: {}", path.display()));
            return;
        }
        self.after_playlist_edit();
    }

    pub fn import_m3u_into(&mut self, path: &Path, list: usize) {
        match m3u::read(path) {
            Ok(entries) => {
                let tracks: Vec<Track> = entries
                    .iter()
                    .map(|e| {
                        if e.path.exists() {
                            self.track_for(&e.path)
                        } else {
                            let mut t = Track::from_path(e.path.clone());
                            t.title = e.title.clone();
                            t.duration_ms = e.duration_secs.map(|s| s as u64 * 1000);
                            t
                        }
                    })
                    .collect();
                self.playlists[list].add(tracks);
            }
            Err(e) => self.toast(format!("Cannot read {}: {e}", path.display())),
        }
    }

    pub fn import_m3u_as_new(&mut self, path: &Path) {
        let path = expand_tilde(path);
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported".into());
        let idx = self.new_playlist(name);
        self.import_m3u_into(&path, idx);
        self.after_playlist_edit();
    }

    pub fn export_m3u(&mut self, path: &Path) {
        let path = expand_tilde(path);
        let pl = &self.playlists[self.active];
        match m3u::write(&path, &pl.tracks) {
            Ok(()) => self.toast(format!("Saved {}", path.display())),
            Err(e) => self.toast(format!("Cannot write {}: {e}", path.display())),
        }
    }

    pub fn unique_name(&self, base: &str) -> String {
        let base = if base.trim().is_empty() { "Playlist" } else { base.trim() };
        if !self.playlists.iter().any(|p| p.name == base) {
            return base.to_string();
        }
        (2..)
            .map(|n| format!("{base} {n}"))
            .find(|n| !self.playlists.iter().any(|p| &p.name == n))
            .expect("unbounded")
    }

    pub fn new_playlist(&mut self, name: String) -> usize {
        let mut p = Playlist::new(self.unique_name(&name));
        p.dirty = true;
        self.playlists.push(p);
        self.active = self.playlists.len() - 1;
        self.mark_cfg();
        self.active
    }

    pub fn rename_playlist(&mut self, idx: usize, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() || idx >= self.playlists.len() || self.playlists[idx].name == name {
            return;
        }
        let name = self.unique_name(&name);
        self.playlists[idx].name = name;
        self.playlists[idx].dirty = true;
        self.mark_cfg();
    }

    pub fn delete_playlist(&mut self, idx: usize) {
        if idx >= self.playlists.len() {
            return;
        }
        let p = self.playlists.remove(idx);
        if let (Some(db), Some(id)) = (&self.db, p.id)
            && let Err(e) = db::delete_playlist(db, id)
        {
            tracing::warn!("delete playlist: {e}");
        }
        if self.playlists.is_empty() {
            let mut d = Playlist::new("Default");
            d.dirty = true;
            self.playlists.push(d);
        }
        let fix = |i: usize| if i > idx { i - 1 } else { i };
        self.active = fix(self.active).min(self.playlists.len() - 1);
        if self.playing_list == idx {
            // The track keeps playing; next/prev continue in the visible list.
            self.playing_list = self.active;
            self.audio.send(Command::PrefetchNext(None));
        } else {
            self.playing_list = fix(self.playing_list);
        }
        // Positions of the remaining lists changed.
        for p in &mut self.playlists {
            p.dirty = true;
        }
        self.mark_cfg();
    }

    /// Call after any change to playlist contents.
    pub fn after_playlist_edit(&mut self) {
        self.mark_cfg();
        self.refresh_prefetch();
    }

    pub fn remove_selected(&mut self) {
        // Removing the playing row keeps it playing; the playlist then continues
        // from the row that followed it, which after_playlist_edit prefetches.
        self.playlists[self.active].remove_selected();
        self.after_playlist_edit();
    }

    pub fn add_library_tracks(&mut self, idx: &[usize], replace_and_play: bool) {
        let tracks: Vec<Track> = idx
            .iter()
            .filter_map(|&i| self.library.get(i).cloned())
            .collect();
        if tracks.is_empty() {
            return;
        }
        let list = self.active;
        if replace_and_play {
            self.playlists[list].clear();
        }
        let start = self.playlists[list].len();
        self.playlists[list].add(tracks);
        self.after_playlist_edit();
        if replace_and_play {
            self.play_index(list, start);
        }
    }

    pub fn rescan(&mut self) {
        if self.scan.is_some() {
            return;
        }
        let roots: Vec<PathBuf> = self
            .cfg
            .library_roots
            .iter()
            .map(|r| expand_tilde(r))
            .filter(|r| r.is_dir())
            .collect();
        if roots.is_empty() {
            self.toast("No library folders found (library_roots in config.toml)");
            return;
        }
        self.scan_progress = Some((0, 0));
        self.scan = Some(scanner::start(
            roots,
            db::default_path(),
            self.ctx.clone(),
        ));
    }

}

impl App {
    // ---------------------------------------------------------------- events

    fn handle_audio_events(&mut self) {
        while let Ok(ev) = self.audio.events.try_recv() {
            match ev {
                Event::Loaded(info) => self.on_track_started(info, false),
                Event::Advanced(info) => self.on_track_started(info, true),
                Event::StateChanged(s) => {
                    if s == PlayerState::Playing && self.state != s {
                        // The marquee holds still when not playing; restart its scroll.
                        self.marquee_start = Instant::now();
                    }
                    self.state = s;
                    self.mpris.send(MprisUpdate::Status(s));
                }
                Event::TrackEnded => {
                    let (repeat, shuffle) = (self.cfg.repeat, self.cfg.shuffle);
                    if let Some(i) = self.playing().next(repeat, shuffle, true) {
                        self.play_index(self.playing_list, i);
                    }
                }
                Event::Seeked(pos) => self.mpris.send(MprisUpdate::Seeked(pos)),
                Event::LoadFailed { path, msg, play } => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    self.toast(format!("{name}: {msg}"));
                    self.failed.insert(path.clone());
                    if play {
                        self.skip_failed(&path);
                    }
                }
                Event::Error { path, msg } => {
                    let name = path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned());
                    match name {
                        Some(n) => self.toast(format!("{n}: {msg}")),
                        None => self.toast(msg),
                    }
                }
            }
        }
    }

    /// Moves on from a track that could not be loaded, unless every track of
    /// the list has failed in a row.
    fn skip_failed(&mut self, path: &Path) {
        let pl = self.playing();
        if pl.current_track().is_none_or(|t| t.path != path) {
            return;
        }
        let len = pl.len();
        self.load_failures += 1;
        if self.load_failures < len {
            self.next();
        } else {
            self.load_failures = 0;
        }
    }

    fn on_track_started(&mut self, info: TrackInfo, gapless: bool) {
        self.load_failures = 0;
        self.failed.remove(&info.path);
        if gapless && self.playing_list < self.playlists.len() {
            // The engine moved on by itself; advance the playlist pointer to match.
            let (repeat, shuffle) = (self.cfg.repeat, self.cfg.shuffle);
            let pl = self.playing();
            let expected = pl.peek_next(repeat, shuffle, true);
            if expected.and_then(|i| pl.tracks.get(i)).map(|t| &t.path) == Some(&info.path) {
                pl.next(repeat, shuffle, true);
            } else if let Some(i) = pl.tracks.iter().position(|t| t.path == info.path) {
                pl.set_current(i);
            }
        }
        let track = self
            .playlists
            .get(self.playing_list)
            .and_then(|p| p.current_track())
            .filter(|t| t.path == info.path)
            .cloned()
            .unwrap_or_else(|| self.track_for(&info.path));
        self.track_counter += 1;
        self.mpris.send(MprisUpdate::Track {
            id: self.track_counter,
            title: track.display_title(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            length: info.duration.or(track.duration()),
            path: info.path.clone(),
        });
        self.auto_eq_for(&info.path);
        self.now_track = Some(track);
        self.now = Some(info);
        self.marquee_start = Instant::now();
        self.scroll_to = self
            .playlists
            .get(self.playing_list)
            .filter(|_| self.playing_list == self.active)
            .and_then(|p| p.current);
        self.mark_cfg();
        self.refresh_prefetch();
    }

    fn handle_scan_events(&mut self) {
        let Some(rx) = &self.scan else { return };
        let events: Vec<ScanEvent> = rx.try_iter().collect();
        for ev in events {
            match ev {
                ScanEvent::Progress { done, total } => self.scan_progress = Some((done, total)),
                ScanEvent::Done {
                    tracks,
                    updated,
                    removed,
                } => {
                    self.library = tracks;
                    self.library_gen += 1;
                    self.refresh_playlist_metadata();
                    if updated + removed > 0 {
                        tracing::info!("library: {updated} updated, {removed} removed");
                    }
                    self.scan = None;
                    self.scan_progress = None;
                }
                ScanEvent::Error(e) => {
                    self.toast(format!("Library scan failed: {e}"));
                    self.scan = None;
                    self.scan_progress = None;
                }
            }
        }
    }

    /// Replaces playlist entries' metadata with fresh library data after a scan.
    fn refresh_playlist_metadata(&mut self) {
        let by_path: std::collections::HashMap<&Path, &Track> =
            self.library.iter().map(|t| (t.path.as_path(), t)).collect();
        for pl in &mut self.playlists {
            for t in &mut pl.tracks {
                if let Some(fresh) = by_path.get(t.path.as_path())
                    && *fresh != t
                {
                    *t = (*fresh).clone();
                }
            }
        }
        if let Some(now) = &self.now
            && let Some(fresh) = by_path.get(now.path.as_path())
        {
            self.now_track = Some((*fresh).clone());
        }
    }

    fn handle_mpris(&mut self, ctx: &egui::Context) {
        while let Ok(a) = self.mpris.actions.try_recv() {
            match a {
                MprisAction::PlayPause => {
                    if self.state == PlayerState::Playing {
                        self.audio.send(Command::Pause);
                    } else {
                        self.play();
                    }
                }
                // Unlike the X key, MPRIS Play must not restart a playing track.
                MprisAction::Play => {
                    if self.state != PlayerState::Playing {
                        self.play();
                    }
                }
                MprisAction::Pause => self.audio.send(Command::Pause),
                MprisAction::Stop => self.audio.send(Command::Stop),
                MprisAction::Next => self.next(),
                MprisAction::Previous => self.prev(),
                MprisAction::Seek(us) => self.audio.send(Command::SeekRel(us / 1000)),
                MprisAction::SetPosition(us) => self
                    .audio
                    .send(Command::Seek(Duration::from_micros(us.max(0) as u64))),
                MprisAction::Volume(v) => self.set_volume(v as f32),
                MprisAction::Shuffle(v) => self.set_shuffle(v),
                MprisAction::Repeat(r) => self.set_repeat(r),
                MprisAction::Raise => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
                MprisAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        if self.state == PlayerState::Playing
            && self.last_mpris_pos.elapsed() > Duration::from_millis(500)
        {
            self.last_mpris_pos = Instant::now();
            self.mpris.send(MprisUpdate::Position(self.audio.position()));
        }
    }

    fn handle_theme(&mut self, ctx: &egui::Context) {
        let changed = self
            .theme_watch
            .as_ref()
            .is_some_and(|w| w.changed.try_recv().is_ok());
        if changed {
            tracing::debug!("theme files changed, reloading");
            let t = Theme::load();
            if t != self.theme {
                tracing::info!("theme changed: {}", t.name);
                t.apply(ctx);
                self.theme = t;
            }
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        for p in dropped {
            self.add_path(&p);
        }
    }

    pub fn dispatch(&mut self, ctx: &egui::Context, a: Action) {
        match a {
            Action::Prev => self.prev(),
            Action::Play => self.play(),
            Action::PauseToggle => match self.state {
                PlayerState::Playing => self.audio.send(Command::Pause),
                PlayerState::Paused => self.audio.send(Command::Play),
                PlayerState::Stopped => {}
            },
            Action::Stop => self.audio.send(Command::Stop),
            Action::Next => self.next(),
            Action::TogglePlay => {
                if self.state == PlayerState::Playing {
                    self.audio.send(Command::Pause);
                } else {
                    self.play();
                }
            }
            Action::SeekRel(ms) => self.audio.send(Command::SeekRel(ms)),
            Action::VolumeDelta(d) => self.set_volume(self.cfg.volume + d),
            Action::CycleRepeat => self.set_repeat(self.cfg.repeat.cycle()),
            Action::ToggleShuffle => self.set_shuffle(!self.cfg.shuffle),
            Action::ToggleEq => {
                self.cfg.show_eq = !self.cfg.show_eq;
                self.resize_window(ctx);
            }
            Action::TogglePlaylist => {
                self.cfg.show_playlist = !self.cfg.show_playlist;
                self.resize_window(ctx);
            }
            Action::ToggleLibrary => {
                self.cfg.show_library = !self.cfg.show_library;
                self.resize_window(ctx);
            }
            Action::FocusOpen => self.focus_open = true,
            Action::Jump => {
                self.jump = Some(JumpState {
                    focus: true,
                    ..Default::default()
                })
            }
            Action::RemoveSelected => self.remove_selected(),
            Action::PlaySelected => {
                if let Some(&i) = self.playlists[self.active].selected.first() {
                    self.play_index(self.active, i);
                }
            }
            Action::SelectAll => self.playlists[self.active].select_all(),
        }
    }

    pub fn resize_window(&mut self, ctx: &egui::Context) {
        // Remember the playlist height so it comes back the same size.
        let size = window_size(&self.cfg);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        self.mark_cfg();
    }

    fn save_now(&mut self) {
        let mut error = None;
        let mut saved = false;
        if let Some(db) = &mut self.db {
            for (i, p) in self.playlists.iter_mut().enumerate() {
                if !p.dirty {
                    continue;
                }
                // Cleared even on failure: retrying every second would not help.
                p.dirty = false;
                match db::save_playlist(db, p, i) {
                    Ok(()) => saved = true,
                    Err(e) => error = Some(format!("Could not save playlist {}: {e}", p.name)),
                }
            }
        }
        match error {
            Some(msg) if !self.save_failed => {
                self.save_failed = true;
                self.toast(msg);
            }
            Some(msg) => tracing::warn!("{msg}"),
            None if saved => self.save_failed = false,
            None => {}
        }
        // Remember the list that is playing, so the next start resumes it.
        let list = if self.now.is_some() { self.playing_list } else { self.active };
        self.cfg.last_playlist = self.playlists.get(list).map(|p| p.name.clone());
        self.cfg.last_track_index = self.playlists.get(list).and_then(|p| p.current);
        self.cfg.last_position_ms = (self.now.is_some() && self.state != PlayerState::Stopped)
            .then(|| self.audio.position().as_millis() as u64);
        if let Err(e) = self.cfg.save() {
            tracing::warn!("save config: {e}");
        }
        self.cfg_changed = None;
        self.cfg_first_change = None;
    }

    fn remember_window(&mut self, ctx: &egui::Context) {
        let h = ctx.content_rect().height().round();
        // Tiled or squeezed heights would reopen a floating window with no room
        // for the playlist; window_size() clamps too, this keeps config sane.
        if self.cfg.show_playlist
            && self.cfg.window.h != Some(h)
            && h >= ui::MAIN_H + ui::PLAYLIST_MIN_H
        {
            self.cfg.window.h = Some(h);
            self.mark_cfg();
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        self.handle_audio_events();
        self.handle_scan_events();
        self.handle_mpris(ctx);
        self.handle_theme(ctx);
        self.handle_drops(ctx);

        if !ctx.egui_wants_keyboard_input() && self.prompt.is_none() && self.jump.is_none() {
            for a in shortcuts::collect(ctx) {
                self.dispatch(ctx, a);
            }
        }

        if let Some(tap) = &mut self.audio.tap {
            self.spectrum.feed(tap);
        }
        let playing = self.state == PlayerState::Playing;
        self.spectrum.update(dt, playing);

        self.toasts.retain(|(_, t)| t.elapsed() < TOAST_TIME);
        self.remember_window(ctx);
        // Without a database playlists are never saved, so ignore their flag.
        if self.cfg_changed.is_none()
            && self.db.is_some()
            && self.playlists.iter().any(|p| p.dirty)
        {
            self.mark_cfg();
        }
        if should_save(self.cfg_first_change, self.cfg_changed, now) {
            self.save_now();
        }

        if playing || !self.spectrum.is_idle() {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else if self.cfg_changed.is_some() || !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::show(self, ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
        self.audio.send(Command::Stop);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.bg.to_normalized_gamma_f32()
    }
}

/// Debounce: save once changes stop for `SAVE_DEBOUNCE`, or after
/// `SAVE_CAP` of continuous changes.
fn should_save(first: Option<Instant>, last: Option<Instant>, now: Instant) -> bool {
    match (first, last) {
        (Some(first), Some(last)) => {
            now.duration_since(last) >= SAVE_DEBOUNCE || now.duration_since(first) >= SAVE_CAP
        }
        _ => false,
    }
}

fn expand_tilde(p: &Path) -> PathBuf {
    if let Ok(rest) = p.strip_prefix("~")
        && let Some(b) = directories::BaseDirs::new()
    {
        return b.home_dir().join(rest);
    }
    p.to_path_buf()
}

fn is_playlist_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_waits_and_caps() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        assert!(!should_save(None, None, ms(5000)));
        // Waits for a quiet second after the last change.
        assert!(!should_save(Some(t0), Some(ms(900)), ms(1500)));
        assert!(should_save(Some(t0), Some(ms(900)), ms(1900)));
        // Changes every 500 ms never go quiet, but the cap saves at 10 s.
        assert!(!should_save(Some(t0), Some(ms(9500)), ms(9900)));
        assert!(should_save(Some(t0), Some(ms(9500)), ms(10_000)));
    }
}
