use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum AddTarget {
    List(usize),
    /// Fill tags of playlist entries that have no cached metadata.
    Fill,
    /// The target playlist was deleted; results are dropped.
    Dropped,
}

pub(super) struct AddRequest {
    pub(super) paths: Vec<PathBuf>,
    pub(super) target: AddTarget,
    pub(super) play: bool,
}

pub(super) struct AddJob {
    rx: Receiver<AddEvent>,
    target: AddTarget,
    play: bool,
    played: bool,
}

impl App {
    pub fn track_for(&self, path: &Path) -> Track {
        self.db
            .as_ref()
            .and_then(|db| db::get_track(db, path).ok().flatten())
            .unwrap_or_else(|| tags::read(path))
    }

    /// Adds files, directories (recursively) and playlist files to the active playlist
    /// off the UI thread. With `play`, starts the first row added.
    pub fn add_paths(&mut self, paths: Vec<PathBuf>, play: bool) {
        let paths = paths.iter().map(|p| expand_tilde(p)).collect();
        self.add_queue.push_back(AddRequest {
            paths,
            target: AddTarget::List(self.active),
            play,
        });
        self.start_next_add();
    }

    pub(super) fn start_next_add(&mut self) {
        if self.add_job.is_some() {
            return;
        }
        if let Some(req) = self.add_queue.pop_front() {
            let rx = adder::start(req.paths, db::default_path(), self.ctx.clone());
            self.add_job = Some(AddJob {
                rx,
                target: req.target,
                play: req.play,
                played: false,
            });
        }
    }

    /// True while an add job runs or requests wait.
    pub fn adding(&self) -> bool {
        self.add_job.is_some() || !self.add_queue.is_empty()
    }

    pub(super) fn handle_add_events(&mut self) {
        let Some(job) = &self.add_job else { return };
        let mut events = Vec::new();
        let mut finished = false;
        loop {
            match job.rx.try_recv() {
                Ok(ev) => events.push(ev),
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        let (target, play) = (job.target, job.play);
        for ev in events {
            match ev {
                AddEvent::Tracks(tracks) => match target {
                    AddTarget::List(i) if i < self.playlists.len() => {
                        let start = self.playlists[i].len();
                        self.playlists[i].add(tracks);
                        self.after_playlist_edit();
                        let played = self.add_job.as_ref().is_some_and(|j| j.played);
                        if play && !played {
                            self.play_index(i, start);
                            if let Some(j) = &mut self.add_job {
                                j.played = true;
                            }
                        }
                    }
                    AddTarget::Fill => self.fill_metadata(tracks),
                    _ => {}
                },
                AddEvent::Problem(msg) => {
                    if target != AddTarget::Fill {
                        self.toast(msg);
                    }
                }
                AddEvent::Done => finished = true,
            }
        }
        if finished {
            self.add_job = None;
            self.start_next_add();
        }
    }

    /// Replaces playlist entries without cached metadata by freshly read tracks.
    pub(super) fn fill_metadata(&mut self, tracks: Vec<Track>) {
        let by_path: std::collections::HashMap<&Path, &Track> =
            tracks.iter().map(|t| (t.path.as_path(), t)).collect();
        // Not marked dirty: playlists store only paths.
        for pl in &mut self.playlists {
            for t in &mut pl.tracks {
                if t.mtime == 0
                    && let Some(fresh) = by_path.get(t.path.as_path())
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

    pub fn import_m3u_as_new(&mut self, path: &Path) {
        let path = expand_tilde(path);
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported".into());
        let idx = self.new_playlist(name);
        self.add_queue.push_back(AddRequest {
            paths: vec![path],
            target: AddTarget::List(idx),
            play: false,
        });
        self.start_next_add();
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
        let base = if base.trim().is_empty() {
            "Playlist"
        } else {
            base.trim()
        };
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
        let retarget = |t: &mut AddTarget| {
            if let AddTarget::List(i) = *t {
                *t = if i == idx {
                    AddTarget::Dropped
                } else {
                    AddTarget::List(fix(i))
                };
            }
        };
        if let Some(j) = &mut self.add_job {
            retarget(&mut j.target);
        }
        for r in &mut self.add_queue {
            retarget(&mut r.target);
        }
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
            .collect();
        if roots.is_empty() {
            self.toast("No library folders configured (library_roots in config.toml)");
            return;
        }
        self.scan_progress = Some((0, 0));
        self.scan = Some(scanner::start(roots, db::default_path(), self.ctx.clone()));
    }
}
