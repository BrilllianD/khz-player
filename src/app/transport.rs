use super::*;
use crate::audio::Command;
use crate::library::ReplayGain;

/// Which backend plays the current track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    #[default]
    Engine,
    Video,
}

/// `Video` only when the file has a video track, video is enabled and libmpv works.
pub fn backend_for(has_video: bool, video_enabled: bool, video_available: bool) -> Backend {
    if has_video && video_enabled && video_available {
        Backend::Video
    } else {
        Backend::Engine
    }
}

/// What the engine keeps internally for a track, kept here for one played by mpv.
pub(super) struct VideoCur {
    pub(super) path: PathBuf,
    /// For a reload after stop or end.
    pub(super) gain: ReplayGain,
    /// mpv holds the file (false after `t_stop`, `Ended`, `WindowClosed`).
    pub(super) loaded: bool,
    /// The last load asked to play (decides skip on a load error).
    pub(super) play: bool,
    /// State to enter on `Started`: Playing, Stopped (load paused) or Paused (resume).
    pub(super) initial: PlayerState,
}

#[derive(Debug, PartialEq, Eq)]
enum Handover {
    Resume { play: bool },
    Load,
}

/// How a track moves from mpv to the engine when video is switched off.
fn handover(state: PlayerState, loaded: bool) -> Handover {
    match (loaded, state) {
        (true, PlayerState::Playing) => Handover::Resume { play: true },
        (true, PlayerState::Paused) => Handover::Resume { play: false },
        _ => Handover::Load,
    }
}

impl App {
    /// Raises the mpv window while a video track is current.
    pub fn show_video(&self) {
        if self.backend == Backend::Video
            && let Some(v) = &self.video
        {
            v.raise();
        }
    }

    /// Settings toggle for video playback. Off while mpv owns the track: the engine takes the
    /// same track over at the same position and state.
    pub fn set_video_enabled(&mut self, on: bool) {
        if self.cfg.video.enabled == on {
            return;
        }
        self.cfg.video.enabled = on;
        self.mark_cfg();
        if on || self.backend != Backend::Video {
            return;
        }
        let Some(cur) = &self.video_cur else {
            self.leave_video();
            return;
        };
        let (path, gain, loaded) = (cur.path.clone(), cur.gain, cur.loaded);
        let at = self.position();
        match handover(self.state, loaded) {
            Handover::Resume { play } => {
                self.t_resume(path, at, gain);
                if play {
                    self.t_play();
                }
            }
            Handover::Load => self.t_load(path, false, gain),
        }
    }

    /// True when `path` plays in the mpv window. Never creates the player.
    pub(super) fn plays_as_video(&self, path: &Path) -> bool {
        let usable = self.cfg.video.enabled && self.video_error.is_none();
        backend_for(usable && tags::has_video(path), self.cfg.video.enabled, self.video_error.is_none())
            == Backend::Video
    }

    /// Picks the backend for `path`, creating the player when needed.
    fn route(&mut self, path: &Path) -> Backend {
        if self.plays_as_video(path) && self.ensure_video() {
            Backend::Video
        } else {
            Backend::Engine
        }
    }

    fn ensure_video(&mut self) -> bool {
        if self.video.is_some() {
            return true;
        }
        match VideoPlayer::new(self.ctx.clone()) {
            Ok(v) => {
                v.set_volume(self.cfg.volume);
                v.set_device(self.cfg.output_device.as_deref());
                self.video = Some(v);
                true
            }
            Err(e) => {
                self.toast(format!(
                    "Video playback unavailable ({e:#}), playing video files as audio"
                ));
                self.video_error = Some(format!("{e:#}"));
                false
            }
        }
    }

    /// Drops the player (closing its window) and hands over to the engine.
    fn leave_video(&mut self) {
        self.video = None;
        self.video_cur = None;
        self.backend = Backend::Engine;
        // The engine was stopped by load_video and may emit no StateChanged
        // (a failed load), so do not keep the video's state.
        self.set_state(PlayerState::Stopped);
    }

    /// Switches to mpv for a new track and loads it.
    fn load_video(&mut self, path: PathBuf, play: bool, at: Duration, gain: ReplayGain, initial: PlayerState) {
        if self.backend == Backend::Engine {
            self.audio.send(Command::Stop);
            self.audio.send(Command::PrefetchNext(None));
        }
        self.backend = Backend::Video;
        if let Some(v) = &self.video {
            v.load(&path, play, at);
        }
        self.video_cur = Some(VideoCur {
            path,
            gain,
            loaded: true,
            play,
            initial,
        });
    }

    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn video_error(&self) -> Option<&str> {
        self.video_error.as_deref()
    }

    /// Track info for a video track, from the library or tags.
    pub(super) fn video_track_info(&self, path: &Path) -> TrackInfo {
        let t = self.track_for(path);
        TrackInfo {
            path: path.to_path_buf(),
            sample_rate: t.sample_rate.unwrap_or(0),
            channels: t.channels.map_or(2, usize::from),
            duration: t.duration(),
            kbps: None,
            codec: "video".into(),
        }
    }

    /// Resume playback of the loaded track.
    pub fn t_play(&mut self) {
        if self.backend != Backend::Video {
            self.audio.send(Command::Play);
            return;
        }
        let Some(cur) = &self.video_cur else { return };
        match self.state {
            PlayerState::Playing => {
                if let Some(v) = &self.video {
                    v.seek(Duration::ZERO);
                }
            }
            PlayerState::Paused => {
                if let Some(v) = &self.video {
                    v.play();
                }
            }
            PlayerState::Stopped => {
                if cur.loaded {
                    if let Some(v) = &self.video {
                        v.play();
                    }
                } else {
                    let (path, gain) = (cur.path.clone(), cur.gain);
                    self.t_load(path, true, gain);
                }
            }
        }
    }

    /// Pause playback.
    pub fn t_pause(&mut self) {
        if self.backend != Backend::Video {
            self.audio.send(Command::Pause);
        } else if self.state == PlayerState::Playing
            && let Some(v) = &self.video
        {
            v.pause();
        }
    }

    /// Stop playback and unload the track.
    pub fn t_stop(&mut self) {
        if self.backend != Backend::Video {
            self.audio.send(Command::Stop);
            return;
        }
        let Some(cur) = &mut self.video_cur else { return };
        cur.loaded = false;
        if let Some(v) = &self.video {
            v.stop();
        }
        // mpv sends no event for a stop.
        self.set_state(PlayerState::Stopped);
    }

    /// Seek to an absolute position.
    pub fn t_seek(&mut self, to: Duration) {
        if self.backend != Backend::Video {
            self.audio.send(Command::Seek(to));
        } else if self.video_cur.as_ref().is_some_and(|c| c.loaded)
            && let Some(v) = &self.video
        {
            v.seek(to);
        }
    }

    /// Seek relative to the current position, in milliseconds.
    pub fn t_seek_rel(&mut self, ms: i64) {
        if self.backend != Backend::Video {
            self.audio.send(Command::SeekRel(ms));
        } else if self.video_cur.as_ref().is_some_and(|c| c.loaded)
            && let Some(v) = &self.video
        {
            v.seek_rel(ms);
        }
    }

    /// Load a track, playing it at once when `play` is set.
    pub fn t_load(&mut self, path: PathBuf, play: bool, gain: ReplayGain) {
        match self.route(&path) {
            Backend::Engine => {
                if self.backend == Backend::Video {
                    self.leave_video();
                }
                self.audio.send(Command::Load { path, play, gain });
            }
            Backend::Video => {
                let initial = if play { PlayerState::Playing } else { PlayerState::Stopped };
                self.load_video(path, play, Duration::ZERO, gain, initial);
            }
        }
    }

    /// Load a track paused at position `at`.
    pub fn t_resume(&mut self, path: PathBuf, at: Duration, gain: ReplayGain) {
        match self.route(&path) {
            Backend::Engine => {
                if self.backend == Backend::Video {
                    self.leave_video();
                }
                self.audio.send(Command::Resume { path, at, gain });
            }
            Backend::Video => self.load_video(path, false, at, gain, PlayerState::Paused),
        }
    }

    /// Tell the engine which track to prefetch for gapless playback (engine only).
    pub fn t_prefetch(&mut self, next: Option<(PathBuf, ReplayGain)>) {
        self.audio.send(Command::PrefetchNext(next));
    }

    /// Current playback position.
    pub fn position(&self) -> Duration {
        match (&self.video, self.backend) {
            (Some(v), Backend::Video) => v.position(),
            _ => self.audio.position(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handover_rules() {
        use PlayerState::*;
        assert_eq!(handover(Playing, true), Handover::Resume { play: true });
        assert_eq!(handover(Paused, true), Handover::Resume { play: false });
        assert_eq!(handover(Stopped, true), Handover::Load);
        assert_eq!(handover(Playing, false), Handover::Load);
        assert_eq!(handover(Paused, false), Handover::Load);
        assert_eq!(handover(Stopped, false), Handover::Load);
    }

    #[test]
    fn video_only_when_all_three() {
        for bits in 0..8u8 {
            let (a, b, c) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
            let want = if a && b && c { Backend::Video } else { Backend::Engine };
            assert_eq!(backend_for(a, b, c), want, "{a} {b} {c}");
        }
    }
}
