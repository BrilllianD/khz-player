use super::*;
use crate::audio::Command;
use crate::library::ReplayGain;

impl App {
    /// Resume playback of the loaded track.
    pub fn t_play(&mut self) {
        self.audio.send(Command::Play);
    }

    /// Pause playback.
    pub fn t_pause(&mut self) {
        self.audio.send(Command::Pause);
    }

    /// Stop playback and unload the track.
    pub fn t_stop(&mut self) {
        self.audio.send(Command::Stop);
    }

    /// Seek to an absolute position.
    pub fn t_seek(&mut self, to: Duration) {
        self.audio.send(Command::Seek(to));
    }

    /// Seek relative to the current position, in milliseconds.
    pub fn t_seek_rel(&mut self, ms: i64) {
        self.audio.send(Command::SeekRel(ms));
    }

    /// Load a track, playing it at once when `play` is set.
    pub fn t_load(&mut self, path: PathBuf, play: bool, gain: ReplayGain) {
        self.audio.send(Command::Load { path, play, gain });
    }

    /// Load a track paused at position `at`.
    pub fn t_resume(&mut self, path: PathBuf, at: Duration, gain: ReplayGain) {
        self.audio.send(Command::Resume { path, at, gain });
    }

    /// Tell the engine which track to prefetch for gapless playback (engine only).
    pub fn t_prefetch(&mut self, next: Option<(PathBuf, ReplayGain)>) {
        self.audio.send(Command::PrefetchNext(next));
    }

    /// Current playback position.
    pub fn position(&self) -> Duration {
        self.audio.position()
    }
}
