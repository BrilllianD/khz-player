use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use rusqlite::Connection;

use crate::audio::spectrum::Spectrum;
use crate::audio::{AudioHandle, Event, PlayerState, TrackInfo};
use crate::config::{Config, Visualization};
use crate::dialog::{self, Purpose};
use crate::eq_presets::{self, Preset};
use crate::job::{Job, Poll};
use crate::library::adder::{self, AddEvent};
use crate::library::scanner::{self, ScanEvent};
use crate::library::{Track, db, tags};
use crate::mpris::{self, Caps, Mpris, MprisAction, MprisUpdate};
use crate::playlist::Playlist;
use crate::shortcuts::{self, Action};
use crate::theme::Theme;
use crate::theme_watch::{self, ThemeWatcher};
use crate::ui;
use crate::ui::playlist_panel::ScrollMode;
use crate::video::{VideoEvent, VideoPlayer};
use crate::{fonts, m3u};


mod events;
mod eq;
mod persist;
mod playback;
mod playlists;
mod transport;

use persist::should_save;
use playlists::{AddJob, AddRequest, AddTarget};

pub const DOUBLE_ZOOM: f32 = 2.0;
/// Longest wait after a zoom change for the window to reach its target size.
const ZOOM_SETTLE: Duration = Duration::from_secs(2);
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

/// Where the tracks of an add request go.
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
    /// mpv window player, created on the first video track.
    pub video: Option<VideoPlayer>,
    /// Which backend owns the current track.
    backend: transport::Backend,
    video_cur: Option<transport::VideoCur>,
    /// Why video playback is unavailable; set once per session.
    video_error: Option<String>,
    /// Last attempt to reopen the audio output.
    last_reopen: Option<Instant>,
    /// The user was told the output is gone; cleared when it is back.
    reopen_failing: bool,
    pub spectrum: Spectrum,
    pub state: PlayerState,
    /// Decoder info of the loaded track.
    pub now: Option<TrackInfo>,
    /// Stop when the current track ends; cleared when it fires.
    stop_after: bool,
    sleep: playback::Sleep,
    /// Tag info of the loaded track (for the marquee).
    pub now_track: Option<Track>,
    track_counter: u64,
    /// Cover of the current track (C7); None until its lookup answers.
    pub cover: Option<PathBuf>,
    pub cover_tex: crate::ui::cover::CoverTexture,
    pub info_cover: crate::ui::cover::CoverTexture,
    cover_tx: crossbeam_channel::Sender<(u64, PathBuf, Option<PathBuf>)>,
    cover_rx: Receiver<(u64, PathBuf, Option<PathBuf>)>,
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
    scan: Option<Job<ScanEvent>>,
    pub scan_progress: Option<(usize, usize)>,
    pub library_search: String,
    pub library_cache: ui::library_panel::Cache,

    dialog: Option<Receiver<dialog::Picked>>,
    add_queue: std::collections::VecDeque<AddRequest>,
    add_job: Option<AddJob>,

    mpris: Mpris,
    last_mpris_pos: Instant,
    mpris_caps: Option<Caps>,

    pub builtin_presets: Vec<Preset>,
    pub user_presets: Vec<Preset>,

    pub prompt: Option<Prompt>,
    pub settings_open: bool,
    pub jump: Option<JumpState>,
    pub info: Option<Track>,
    pub toasts: Vec<(String, Instant)>,
    /// Scroll the playlist to this row on the next frame.
    pub scroll_to: Option<(usize, ui::playlist_panel::ScrollMode)>,
    /// The playlist has keyboard focus (set by clicks in its panel).
    pub playlist_focus: bool,
    /// Rows that fit in the playlist view; written by the panel each frame.
    pub playlist_page_rows: usize,
    last_frame: Instant,
    zoom_pending: bool,
    zoom_settle: Option<Instant>,
}

pub fn window_size(cfg: &Config) -> egui::Vec2 {
    let main_w = cfg.window.w.map_or(ui::MAIN_W, |w| w.max(ui::MAIN_W));
    let w = main_w + if cfg.show_library { cfg.window.library_w.unwrap_or(ui::LIBRARY_W).max(260.0) } else { 0.0 };
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

pub fn zoom(cfg: &Config) -> f32 {
    if cfg.double_size { DOUBLE_ZOOM } else { 1.0 }
}

/// Creation size and minimum size, in physical-scale points (1x sizes times the zoom).
pub fn initial_viewport(cfg: &Config) -> (egui::Vec2, egui::Vec2) {
    let z = zoom(cfg);
    (window_size(cfg) * z, egui::vec2(ui::MAIN_W, ui::MAIN_H) * z)
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Config,
        args: Vec<PathBuf>,
        cfg_error: Option<String>,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        ctx.set_zoom_factor(zoom(&cfg));
        let nerd_font = fonts::install(&ctx, cfg.font_path.as_deref());
        let theme = if cfg.follow_omarchy_theme { Theme::load() } else { Theme::winamp() };
        theme.apply(&ctx);
        let theme_watch = theme_watch::start(ctx.clone());

        let audio = AudioHandle::start(cfg.volume, cfg.balance, cfg.output_device.as_deref(), ctx.clone());
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

        let (cover_tx, cover_rx) = crossbeam_channel::unbounded();
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
            video: None,
            backend: transport::Backend::Engine,
            video_cur: None,
            video_error: None,
            last_reopen: None,
            reopen_failing: false,
            spectrum,
            state: PlayerState::Stopped,
            now: None,
            stop_after: false,
            sleep: playback::Sleep::Off,
            now_track: None,
            track_counter: 0,
            cover: None,
            cover_tex: crate::ui::cover::CoverTexture::new("cover"),
            info_cover: crate::ui::cover::CoverTexture::new("info-cover"),
            cover_tx,
            cover_rx,
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
            dialog: None,
            scan_progress: None,
            library_search: String::new(),
            library_cache: Default::default(),
            add_queue: Default::default(),
            add_job: None,
            mpris,
            last_mpris_pos: Instant::now(),
            mpris_caps: None,
            prompt: None,
            settings_open: false,
            jump: None,
            info: None,
            toasts: Vec::new(),
            scroll_to: None,
            playlist_focus: false,
            playlist_page_rows: 1,
            last_frame: Instant::now(),
            zoom_pending: false,
            zoom_settle: None,
        };
        app.push_eq();
        app.publish_replay_gain();
        app.mpris.send(MprisUpdate::Volume(app.cfg.volume as f64));
        app.mpris.send(MprisUpdate::Shuffle(app.cfg.shuffle));
        app.mpris.send(MprisUpdate::Repeat(app.cfg.repeat));
        // The bad file was set aside; write the loaded values back soon so a
        // crash before exit does not leave only the `.broken` copy.
        if cfg_error.is_some() {
            app.mark_cfg();
        }
        for e in cfg_error.into_iter().chain(presets_error) {
            app.toast(e);
        }
        if let Some(e) = app.audio.init_error.clone() {
            app.toast(format!("Audio output unavailable: {e}"));
            app.reopen_failing = true;
        }
        if app.audio.device_fallback {
            app.toast(events::DEVICE_FALLBACK_MSG);
        }

        if !args.is_empty() {
            app.add_paths(args, true);
        } else if let Some(t) = app.playlists[active].current_track() {
            // Restore the last track without starting playback; paused at the
            // old position if khz-player quit mid-track.
            let path = t.path.clone();
            let gain = t.replay_gain;
            match app.cfg.last_position_ms {
                Some(ms) => app.t_resume(path, Duration::from_millis(ms), gain),
                None => app.t_load(path, false, gain),
            }
        }
        // Entries outside the library have no cached tags; read them off the UI thread,
        // after the command-line request so its first file plays first.
        let mut untagged: Vec<PathBuf> = Vec::new();
        let mut seen: std::collections::HashSet<&Path> = std::collections::HashSet::new();
        for t in app.playlists.iter().flat_map(|p| p.tracks.iter()) {
            if t.mtime == 0 && seen.insert(t.path.as_path()) {
                untagged.push(t.path.clone());
            }
        }
        if !untagged.is_empty() {
            app.add_queue.push_back(AddRequest {
                paths: untagged,
                target: AddTarget::Fill,
                play: false,
            });
            app.start_next_add();
        }
        if needs_rescan(&app.library) {
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

    pub fn set_follow_theme(&mut self, on: bool) {
        self.cfg.follow_omarchy_theme = on;
        let theme = if on { Theme::load() } else { Theme::winamp() };
        theme.apply(&self.ctx);
        self.theme = theme;
        self.mark_cfg();
    }

    pub fn dispatch(&mut self, ctx: &egui::Context, a: Action) {
        match a {
            Action::Prev => self.prev(),
            Action::Play => self.play(),
            Action::PauseToggle => match self.state {
                PlayerState::Playing => self.t_pause(),
                PlayerState::Paused => self.t_play(),
                PlayerState::Stopped => {}
            },
            Action::Stop => self.t_stop(),
            Action::Next => self.next(),
            Action::TogglePlay => {
                if self.state == PlayerState::Playing {
                    self.t_pause();
                } else {
                    self.play();
                }
            }
            Action::SeekRel(ms) => self.t_seek_rel(ms),
            Action::VolumeDelta(d) => self.set_volume(self.cfg.volume + d),
            Action::CycleRepeat => self.set_repeat(self.cfg.repeat.cycle()),
            Action::ToggleShuffle => self.set_shuffle(!self.cfg.shuffle),
            Action::ToggleEq => {
                self.cfg.show_eq = !self.cfg.show_eq;
                self.resize_window(ctx);
            }
            Action::TogglePlaylist => {
                self.cfg.show_playlist = !self.cfg.show_playlist;
                if !self.cfg.show_playlist {
                    self.playlist_focus = false;
                }
                self.resize_window(ctx);
            }
            Action::ToggleLibrary => {
                self.cfg.show_library = !self.cfg.show_library;
                self.resize_window(ctx);
            }
            Action::Cursor(mv, extend) => {
                let rows = self.playlist_page_rows;
                if let Some(c) = self.playlists[self.active].move_cursor(mv, rows, extend) {
                    self.scroll_to = Some((c, ScrollMode::Nearest));
                }
            }
            Action::MoveSelected(dir) => {
                if self.playlists[self.active].nudge_selected(dir) {
                    self.after_playlist_edit();
                    let pl = &self.playlists[self.active];
                    if let Some(c) = pl.cursor.or_else(|| pl.selected.first().copied()) {
                        self.scroll_to = Some((c, ScrollMode::Nearest));
                    }
                }
            }
            Action::AddFiles => self.open_dialog(Purpose::AddFiles),
            Action::Jump => {
                self.jump = Some(JumpState {
                    focus: true,
                    ..Default::default()
                })
            }
            Action::RemoveSelected => self.remove_selected(),
            Action::EnqueueSelected => self.enqueue_selected(),
            Action::StopAfterCurrent => self.toggle_stop_after(),
            Action::SleepTimer => self.toggle_sleep(),
            Action::PlaySelected => {
                let pl = &self.playlists[self.active];
                let row = pl
                    .cursor
                    .filter(|&c| c < pl.len())
                    .or_else(|| pl.selected.first().copied());
                if let Some(i) = row {
                    self.play_index(self.active, i);
                }
            }
            Action::FileInfo => {
                let pl = &self.playlists[self.active];
                let row = pl
                    .cursor
                    .filter(|&c| c < pl.len())
                    .or_else(|| pl.selected.first().copied());
                let t = row
                    .and_then(|i| pl.tracks.get(i).cloned())
                    .or_else(|| self.now_track.clone());
                if let Some(t) = t {
                    self.open_info(t);
                }
            }
            Action::SelectAll => self.playlists[self.active].select_all(),
            Action::Settings => self.settings_open = true,
            Action::ToggleTimeDisplay => {
                self.cfg.time_remaining = !self.cfg.time_remaining;
                self.mark_cfg();
            }
            Action::ToggleDoubleSize => {
                self.cfg.double_size = !self.cfg.double_size;
                self.mark_cfg();
            }
        }
    }

    pub fn open_info(&mut self, t: Track) {
        self.info = Some(t);
    }

    pub fn cycle_visualization(&mut self) {
        self.cfg.visualization = self.cfg.visualization.next();
        self.mark_cfg();
    }

    pub fn resize_window(&mut self, ctx: &egui::Context) {
        // Remember the playlist height so it comes back the same size.
        let size = window_size(&self.cfg);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        self.mark_cfg();
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        self.handle_audio_events();
        self.handle_video_events();
        self.handle_cover_events();
        let want = self.cover.as_deref().filter(|_| self.cfg.show_cover);
        self.cover_tex.sync(ctx, want);
        self.handle_output_loss(now);
        self.handle_sleep(now);
        self.handle_scan_events();
        self.handle_add_events();
        self.handle_dialog();
        self.handle_mpris(ctx);
        self.handle_theme(ctx);
        self.handle_drops(ctx);

        if !ctx.egui_wants_keyboard_input() && self.prompt.is_none()
            && self.jump.is_none()
            && self.info.is_none()
            && !self.settings_open
        {
            for a in shortcuts::collect(ctx, self.playlist_focus) {
                self.dispatch(ctx, a);
            }
        }

        if let Some(tap) = &mut self.audio.tap {
            self.spectrum.feed(tap);
        }
        let playing = self.state == PlayerState::Playing;
        self.spectrum.update(dt, playing && self.cfg.visualization == Visualization::Bars);

        self.toasts.retain(|(_, t)| t.elapsed() < TOAST_TIME);
        let want = zoom(&self.cfg);
        if ctx.zoom_factor() != want {
            ctx.set_zoom_factor(want);
            self.zoom_pending = true;
        } else if self.zoom_pending {
            self.zoom_pending = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(ui::MAIN_W, ui::MAIN_H)));
            self.resize_window(ctx);
            self.zoom_settle = Some(now + ZOOM_SETTLE);
        }
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

        // Animation rate only when visible; a minimized window still ticks
        // slowly for MPRIS position updates and pending saves.
        let minimized = ctx.input(|i| i.viewport().minimized == Some(true));
        if (playing || !self.spectrum.is_idle()) && !minimized {
            ctx.request_repaint_after(Duration::from_secs_f32(1.0 / self.cfg.animation_fps as f32));
        } else if playing || self.cfg_changed.is_some() || !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui::show(self, ui);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_now();
        self.t_stop();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.bg.to_normalized_gamma_f32()
    }
}

/// Scan on start when the library is empty, or when a schema migration marked rows stale
/// (`mtime = -1`, db v6); the stamp check then re-reads exactly those files.
pub(crate) fn needs_rescan(library: &[Track]) -> bool {
    library.is_empty() || library.iter().any(|t| t.mtime == -1)
}

fn expand_tilde(p: &Path) -> PathBuf {
    if let Ok(rest) = p.strip_prefix("~")
        && let Some(b) = directories::BaseDirs::new()
    {
        return b.home_dir().join(rest);
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_rescan_on_empty_or_stale() {
        let t = |m| Track { mtime: m, ..Default::default() };
        assert!(needs_rescan(&[]));
        assert!(!needs_rescan(&[t(5), t(6)]));
        assert!(needs_rescan(&[t(5), t(6), t(-1)]));
        assert!(!needs_rescan(&[t(0)]));
    }

    #[test]
    fn initial_viewport_scales_only_when_double() {
        let mut cfg = Config::default();
        let base = (window_size(&cfg), egui::vec2(ui::MAIN_W, ui::MAIN_H));
        assert_eq!(initial_viewport(&cfg), base);
        cfg.double_size = true;
        assert_eq!(initial_viewport(&cfg), (base.0 * 2.0, base.1 * 2.0));
        assert_eq!(window_size(&cfg), base.0);
    }

    #[test]
    fn window_size_uses_saved_widths() {
        let mut cfg = Config { show_library: true, ..Config::default() };
        assert_eq!(window_size(&cfg).x, ui::MAIN_W + ui::LIBRARY_W);
        cfg.window.w = Some(700.0);
        cfg.window.library_w = Some(300.0);
        assert_eq!(window_size(&cfg).x, 1000.0);
        cfg.window.w = Some(100.0);
        assert_eq!(window_size(&cfg).x, ui::MAIN_W + 300.0);
        cfg.show_library = false;
        assert_eq!(window_size(&cfg).x, ui::MAIN_W);
    }
}
