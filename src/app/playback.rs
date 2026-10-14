use super::*;

impl App {
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

    pub(super) fn playing(&mut self) -> &mut Playlist {
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
        if self.state == PlayerState::Playing && self.audio.position() > Duration::from_secs(5) {
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
            let idx = pl.selected.first().copied().or(pl.current).unwrap_or(0);
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
        let b = if b.abs() < 0.04 {
            0.0
        } else {
            b.clamp(-1.0, 1.0)
        };
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
}
