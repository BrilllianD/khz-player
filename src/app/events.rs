use super::*;

/// Minimum time between attempts to reopen a lost audio output.
const REOPEN_INTERVAL: Duration = Duration::from_secs(2);

impl App {
    pub(super) fn handle_audio_events(&mut self) {
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
                    self.mpris
                        .send(MprisUpdate::Position(if s == PlayerState::Stopped {
                            Duration::ZERO
                        } else {
                            self.audio.position()
                        }));
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

    /// Reopens the audio output after the device went away or never opened,
    /// at most every `REOPEN_INTERVAL`. The engine keeps its track and state.
    pub(super) fn handle_output_loss(&mut self, now: Instant) {
        let lost = self
            .audio
            .shared
            .device_lost
            .load(std::sync::atomic::Ordering::Acquire);
        if !lost && self.audio.init_error.is_none() {
            return;
        }
        // Nothing else may repaint while paused or stopped.
        self.ctx.request_repaint_after(REOPEN_INTERVAL);
        if self
            .last_reopen
            .is_some_and(|t| now.duration_since(t) < REOPEN_INTERVAL)
        {
            return;
        }
        self.last_reopen = Some(now);
        match self.audio.reopen() {
            Ok(rate) => {
                self.reopen_failing = false;
                self.spectrum = Spectrum::new(rate);
                self.refresh_prefetch();
                self.toast("Audio output restored");
            }
            Err(e) => {
                tracing::debug!("reopen audio output: {e:#}");
                if !self.reopen_failing {
                    self.reopen_failing = true;
                    self.toast("Audio device lost, retrying");
                }
            }
        }
    }

    /// Moves on from a track that could not be loaded, unless every track of
    /// the list has failed in a row.
    pub(super) fn skip_failed(&mut self, path: &Path) {
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

    pub(super) fn on_track_started(&mut self, info: TrackInfo, gapless: bool) {
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
        self.sync_title();
        self.marquee_start = Instant::now();
        self.scroll_to = self
            .playlists
            .get(self.playing_list)
            .filter(|_| self.playing_list == self.active)
            .and_then(|p| p.current);
        self.mark_cfg();
        self.refresh_prefetch();
    }

    pub(super) fn handle_scan_events(&mut self) {
        let Some(rx) = &self.scan else { return };
        let events: Vec<ScanEvent> = rx.try_iter().collect();
        for ev in events {
            match ev {
                ScanEvent::Progress { done, total } => self.scan_progress = Some((done, total)),
                ScanEvent::Done {
                    tracks,
                    updated,
                    removed,
                    errors,
                } => {
                    if let Some(first) = errors.first() {
                        self.toast(format!(
                            "Library scan: {} problem(s), first: {first}",
                            errors.len()
                        ));
                    }
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
    pub(super) fn refresh_playlist_metadata(&mut self) {
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
            self.sync_title();
        }
    }

    /// Window title follows the current track, like Winamp's "Artist - Title - Winamp".
    pub(super) fn sync_title(&self) {
        let title = match &self.now_track {
            Some(t) => format!("{} - khz-player", t.display_name()),
            None => "khz-player".into(),
        };
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    pub(super) fn handle_mpris(&mut self, ctx: &egui::Context) {
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
                MprisAction::Seek(us) => {
                    let len = self.now.as_ref().and_then(|n| n.duration);
                    if mpris::seek_past_end(self.audio.position(), us, len) {
                        self.next();
                    } else {
                        self.audio.send(Command::SeekRel(us / 1000));
                    }
                }
                MprisAction::SetPosition(us) => self
                    .audio
                    .send(Command::Seek(Duration::from_micros(us.max(0) as u64))),
                MprisAction::Volume(v) => self.set_volume(v as f32),
                MprisAction::Shuffle(v) => self.set_shuffle(v),
                MprisAction::Repeat(r) => self.set_repeat(r),
                MprisAction::Raise => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
                MprisAction::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                MprisAction::OpenUri(uri) => match mpris::path_from_file_url(&uri) {
                    Some(p) => self.add_paths(vec![p], false),
                    None => {
                        self.toast(format!("Cannot open {uri}: only local files are supported"))
                    }
                },
            }
        }
        if self.state == PlayerState::Playing
            && self.last_mpris_pos.elapsed() > Duration::from_millis(500)
        {
            self.last_mpris_pos = Instant::now();
            self.mpris
                .send(MprisUpdate::Position(self.audio.position()));
        }
        let nonempty = |l: Option<&Playlist>| l.is_some_and(|p| !p.is_empty());
        let play_list = self.playlists.get(self.playing_list);
        let caps = Caps {
            play: self.now.is_some() || nonempty(self.playlists.get(self.active)),
            seek: self.state != PlayerState::Stopped
                && self.now.as_ref().is_some_and(|n| n.duration.is_some()),
            next: nonempty(play_list),
            prev: nonempty(play_list),
        };
        if self.mpris_caps != Some(caps) {
            self.mpris_caps = Some(caps);
            self.mpris.send(MprisUpdate::Caps(caps));
        }
    }

    pub(super) fn handle_theme(&mut self, ctx: &egui::Context) {
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

    pub(super) fn handle_drops(&mut self, ctx: &egui::Context) {
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
        self.add_paths(dropped, false);
    }

    /// True while a native dialog is open.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn open_dialog(&mut self, purpose: Purpose) {
        if self.dialog.is_some() {
            return;
        }
        let start_dir = self
            .cfg
            .library_roots
            .first()
            .map(|r| expand_tilde(r))
            .filter(|d| d.is_dir());
        self.dialog = Some(dialog::spawn(purpose, start_dir, self.ctx.clone()));
    }

    pub(super) fn handle_dialog(&mut self) {
        let Some(rx) = &self.dialog else { return };
        match rx.try_recv() {
            Ok(picked) => {
                self.dialog = None;
                match picked.purpose {
                    Purpose::AddFiles | Purpose::AddFolder if !picked.paths.is_empty() => {
                        self.add_paths(picked.paths, false);
                    }
                    _ => {}
                }
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => self.dialog = None,
        }
    }
}
