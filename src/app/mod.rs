use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use rusqlite::Connection;

use crate::audio::spectrum::Spectrum;
use crate::audio::{AudioHandle, Command, Event, PlayerState, TrackInfo};
use crate::config::Config;
use crate::dialog::{self, Purpose};
use crate::eq_presets::{self, Preset};
use crate::library::adder::{self, AddEvent};
use crate::library::scanner::{self, ScanEvent};
use crate::library::{Track, db, tags};
use crate::mpris::{self, Caps, Mpris, MprisAction, MprisUpdate};
use crate::playlist::Playlist;
use crate::shortcuts::{self, Action};
use crate::theme::Theme;
use crate::theme_watch::{self, ThemeWatcher};
use crate::ui;
use crate::{fonts, m3u};


mod events;
mod eq;
mod persist;
mod playback;
mod playlists;

use persist::should_save;
use playlists::{AddJob, AddRequest, AddTarget};

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
    /// Last attempt to reopen the audio output.
    last_reopen: Option<Instant>,
    /// The user was told the output is gone; cleared when it is back.
    reopen_failing: bool,
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

    dialog: Option<Receiver<dialog::Picked>>,
    add_queue: std::collections::VecDeque<AddRequest>,
    add_job: Option<AddJob>,

    mpris: Mpris,
    last_mpris_pos: Instant,
    mpris_caps: Option<Caps>,

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
            last_reopen: None,
            reopen_failing: false,
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
            dialog: None,
            scan_progress: None,
            library_search: String::new(),
            library_cache: Default::default(),
            add_queue: Default::default(),
            add_job: None,
            mpris,
            last_mpris_pos: Instant::now(),
            mpris_caps: None,
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
            app.reopen_failing = true;
        }

        if !args.is_empty() {
            app.add_paths(args, true);
        } else if let Some(t) = app.playlists[active].current_track() {
            // Restore the last track without starting playback; paused at the
            // old position if khz-player quit mid-track.
            let path = t.path.clone();
            app.audio.send(match app.cfg.last_position_ms {
                Some(ms) => Command::Resume { path, at: Duration::from_millis(ms) },
                None => Command::Load { path, play: false },
            });
        }
        // Entries outside the library have no cached tags; read them off the UI thread,
        // after the command-line request so its first file plays first.
        let mut untagged: Vec<PathBuf> = Vec::new();
        for t in app.playlists.iter().flat_map(|p| p.tracks.iter()) {
            if t.mtime == 0 && !untagged.contains(&t.path) {
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
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;

        self.handle_audio_events();
        self.handle_output_loss(now);
        self.handle_scan_events();
        self.handle_add_events();
        self.handle_dialog();
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
        self.audio.send(Command::Stop);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.bg.to_normalized_gamma_f32()
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

#[cfg(test)]
mod tests {
    use super::*;

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
