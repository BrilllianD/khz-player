use super::*;
use crate::audio::shared::{RG_ALBUM, RG_OFF, RG_TRACK};
use crate::config::ReplayGainMode;
use crate::playlist::Repeat;

/// Next row after the current track ended on its own, honouring stop-after-current.
pub(super) fn advance_after_end(
    stop_after: &mut bool,
    pl: &mut Playlist,
    repeat: Repeat,
    shuffle: bool,
) -> Option<usize> {
    if std::mem::take(stop_after) {
        None
    } else {
        pl.next(repeat, shuffle, true)
    }
}

pub const SLEEP_FADE: Duration = Duration::from_secs(10);
const SLEEP_RESTORE_DELAY: Duration = Duration::from_millis(200);
const SLEEP_RESTORE_TIMEOUT: Duration = Duration::from_secs(2);

/// Sleep timer state; advanced once per frame by `sleep_step`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sleep {
    #[default]
    Off,
    Armed { deadline: Instant },
    Fading { since: Instant },
    Pausing { since: Instant },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SleepStep {
    Idle,
    Gain(f32),
    Pause,
    Restore,
}

fn sleep_step(s: Sleep, now: Instant, playing: bool) -> (Sleep, SleepStep) {
    match s {
        Sleep::Off => (Sleep::Off, SleepStep::Idle),
        Sleep::Armed { deadline } => {
            if now < deadline {
                (s, SleepStep::Idle)
            } else if playing {
                (Sleep::Fading { since: now }, SleepStep::Gain(1.0))
            } else {
                (Sleep::Off, SleepStep::Restore)
            }
        }
        Sleep::Fading { since } => {
            let e = now.saturating_duration_since(since);
            if !playing {
                (Sleep::Pausing { since: now }, SleepStep::Idle)
            } else if e >= SLEEP_FADE {
                (Sleep::Pausing { since: now }, SleepStep::Pause)
            } else {
                (s, SleepStep::Gain(fade_gain(e)))
            }
        }
        Sleep::Pausing { since } => {
            let e = now.saturating_duration_since(since);
            if (!playing && e >= SLEEP_RESTORE_DELAY) || e >= SLEEP_RESTORE_TIMEOUT {
                (Sleep::Off, SleepStep::Restore)
            } else {
                (s, SleepStep::Idle)
            }
        }
    }
}

/// Quadratic fade-out: 1.0 at the start, 0.0 after `SLEEP_FADE`.
fn fade_gain(elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / SLEEP_FADE.as_secs_f32()).clamp(0.0, 1.0);
    (1.0 - t) * (1.0 - t)
}

fn armed_label(stop_after: bool, sleep: Sleep, now: Instant) -> Option<String> {
    let mut parts = Vec::new();
    if stop_after {
        parts.push("STOP AFTER".to_string());
    }
    match sleep {
        Sleep::Off => {}
        Sleep::Armed { deadline } => {
            let left = deadline.saturating_duration_since(now);
            let secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
            if left >= Duration::from_secs(60) {
                parts.push(format!("SLEEP {}m", secs.div_ceil(60)));
            } else {
                parts.push(format!("SLEEP {secs}s"));
            }
        }
        Sleep::Fading { .. } | Sleep::Pausing { .. } => parts.push("SLEEP".to_string()),
    }
    if parts.is_empty() { None } else { Some(parts.join(" ")) }
}

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
        let gain = pl.tracks[index].replay_gain;
        self.playing_list = list;
        self.t_load(path, true, gain);
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
        if self.state == PlayerState::Playing && self.position() > Duration::from_secs(5) {
            self.t_seek(Duration::ZERO);
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
            self.t_play();
        }
    }

    /// Re-announces the following track to the engine for gapless playback.
    pub fn refresh_prefetch(&mut self) {
        if self.now.is_none() {
            return;
        }
        if !self.cfg.gapless || self.stop_after {
            self.t_prefetch(None);
            return;
        }
        if self.backend == transport::Backend::Video {
            self.t_prefetch(None);
            return;
        }
        let (repeat, shuffle) = (self.cfg.repeat, self.cfg.shuffle);
        let pl = self.playing();
        let next = pl
            .peek_next(repeat, shuffle, true)
            .and_then(|i| pl.tracks.get(i))
            .map(|t| (t.path.clone(), t.replay_gain));
        let next = next.filter(|(p, _)| !self.plays_as_video(p));
        self.t_prefetch(next);
    }

    pub fn toggle_stop_after(&mut self) {
        self.stop_after = !self.stop_after;
        let on = if self.stop_after { "on" } else { "off" };
        self.toast(format!("Stop after current: {on}"));
        self.refresh_prefetch();
    }

    pub fn toggle_sleep(&mut self) {
        if self.sleep == Sleep::Off {
            let mins = self.cfg.sleep_minutes;
            let deadline = Instant::now() + Duration::from_secs(u64::from(mins) * 60);
            self.sleep = Sleep::Armed { deadline };
            self.toast(format!("Sleep timer: {mins} min"));
        } else {
            self.sleep = Sleep::Off;
            self.audio.shared.sleep_gain.store(1.0);
            self.toast("Sleep timer off");
        }
    }

    pub fn sleep_running(&self) -> bool {
        self.sleep != Sleep::Off
    }

    pub fn armed_label(&self) -> Option<String> {
        armed_label(self.stop_after, self.sleep, Instant::now())
    }

    pub(super) fn handle_sleep(&mut self, now: Instant) {
        let (next, step) = sleep_step(self.sleep, now, self.state == PlayerState::Playing);
        self.sleep = next;
        match step {
            SleepStep::Idle => {}
            SleepStep::Gain(g) => self.audio.shared.sleep_gain.store(g),
            SleepStep::Pause => {
                self.audio.shared.sleep_gain.store(0.0);
                self.t_pause();
                self.toast("Sleep timer: paused");
            }
            SleepStep::Restore => self.audio.shared.sleep_gain.store(1.0),
        }
        match self.sleep {
            Sleep::Off => {}
            Sleep::Armed { deadline } => {
                let wait = deadline.saturating_duration_since(now).min(Duration::from_secs(1));
                self.ctx.request_repaint_after(wait);
            }
            Sleep::Fading { .. } | Sleep::Pausing { .. } => {
                self.ctx.request_repaint_after(Duration::from_millis(33));
            }
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        let Some(v) = volume_input(v) else { return };
        self.cfg.volume = v;
        self.audio.shared.volume.store(v);
        if let Some(video) = &self.video {
            video.set_volume(v);
        }
        self.mpris.send(MprisUpdate::Volume(v as f64));
        self.mark_cfg();
    }

    pub fn set_balance(&mut self, b: f32) {
        let Some(b) = balance_input(b) else { return };
        self.cfg.balance = b;
        self.audio.shared.balance.store(b);
        self.mark_cfg();
    }

    pub fn set_shuffle(&mut self, on: bool) {
        self.cfg.shuffle = on;
        self.mpris.send(MprisUpdate::Shuffle(on));
        self.mark_cfg();
        self.publish_replay_gain();
        self.refresh_prefetch();
    }

    /// Hands the ReplayGain settings to the engine; Auto depends on shuffle.
    pub fn publish_replay_gain(&self) {
        let rg = &self.cfg.replaygain;
        let sh = &self.audio.shared;
        sh.rg_mode
            .store(rg_mode_code(rg.mode, self.cfg.shuffle), std::sync::atomic::Ordering::Relaxed);
        sh.rg_preamp_db.store(rg.preamp_db);
        sh.rg_untagged_db.store(rg.untagged_db);
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

/// Engine mode code for the configured mode; Auto plays albums in order
/// and single tracks when shuffled.
fn rg_mode_code(mode: ReplayGainMode, shuffle: bool) -> u8 {
    match mode {
        ReplayGainMode::Off => RG_OFF,
        ReplayGainMode::Track => RG_TRACK,
        ReplayGainMode::Album => RG_ALBUM,
        ReplayGainMode::Auto if shuffle => RG_TRACK,
        ReplayGainMode::Auto => RG_ALBUM,
    }
}

/// Volume to apply, or `None` for a non-finite input (MPRIS may send NaN).
fn volume_input(v: f32) -> Option<f32> {
    v.is_finite().then_some(v.clamp(0.0, 1.0))
}

/// Balance to apply (0.04 dead zone snaps to centre), or `None` for non-finite input.
fn balance_input(b: f32) -> Option<f32> {
    if !b.is_finite() {
        None
    } else if b.abs() < 0.04 {
        Some(0.0)
    } else {
        Some(b.clamp(-1.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Track;

    fn pl3(current: usize) -> Playlist {
        let mut p = Playlist::new("t");
        p.add((0..3).map(|i| Track::from_path(PathBuf::from(format!("/m/{i}.mp3")))));
        p.set_current(current);
        p
    }

    #[test]
    fn advance_after_end_rules() {
        let mut stop = false;
        let mut p = pl3(0);
        assert_eq!(advance_after_end(&mut stop, &mut p, Repeat::Off, false), Some(1));

        let mut stop = true;
        let mut p = pl3(0);
        assert_eq!(advance_after_end(&mut stop, &mut p, Repeat::Off, false), None);
        assert!(!stop);
        assert_eq!(p.current, Some(0));

        let mut stop = false;
        let mut p = pl3(2);
        assert_eq!(advance_after_end(&mut stop, &mut p, Repeat::Off, false), None);
        let mut p = pl3(2);
        assert_eq!(advance_after_end(&mut stop, &mut p, Repeat::All, false), Some(0));
        let mut p = pl3(2);
        assert_eq!(advance_after_end(&mut stop, &mut p, Repeat::One, false), Some(2));

        let mut p = pl3(0);
        p.enqueue(2);
        assert_eq!(advance_after_end(&mut stop, &mut p, Repeat::Off, false), Some(2));
    }

    #[test]
    fn rg_mode_code_maps_modes_and_auto_follows_shuffle() {
        assert_eq!(rg_mode_code(ReplayGainMode::Off, true), RG_OFF);
        assert_eq!(rg_mode_code(ReplayGainMode::Track, false), RG_TRACK);
        assert_eq!(rg_mode_code(ReplayGainMode::Album, true), RG_ALBUM);
        assert_eq!(rg_mode_code(ReplayGainMode::Auto, true), RG_TRACK);
        assert_eq!(rg_mode_code(ReplayGainMode::Auto, false), RG_ALBUM);
    }

    #[test]
    fn volume_and_balance_inputs_drop_non_finite() {
        // The MPRIS `Volume(NaN)` path: `f64::NAN as f32` reaches `set_volume`.
        assert_eq!(volume_input(f64::NAN as f32), None);
        assert_eq!(volume_input(f32::INFINITY), None);
        assert_eq!(volume_input(1.5), Some(1.0));
        assert_eq!(volume_input(0.3), Some(0.3));
        assert_eq!(balance_input(f32::NAN), None);
        assert_eq!(balance_input(0.03), Some(0.0));
        assert_eq!(balance_input(-7.0), Some(-1.0));
        assert_eq!(balance_input(0.5), Some(0.5));
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn sleep_armed_waits_then_fades() {
        let t0 = Instant::now();
        let s = Sleep::Armed { deadline: t0 + secs(60) };
        assert_eq!(sleep_step(s, t0, true), (s, SleepStep::Idle));
        let at = t0 + secs(60);
        assert_eq!(
            sleep_step(s, at, true),
            (Sleep::Fading { since: at }, SleepStep::Gain(1.0))
        );
        assert_eq!(sleep_step(Sleep::Off, t0, true), (Sleep::Off, SleepStep::Idle));
    }

    #[test]
    fn sleep_deadline_while_not_playing_ends() {
        let t0 = Instant::now();
        let s = Sleep::Armed { deadline: t0 };
        assert_eq!(sleep_step(s, t0, false), (Sleep::Off, SleepStep::Restore));
    }

    #[test]
    fn sleep_fade_reaches_pause_after_ten_seconds() {
        let t0 = Instant::now();
        let s = Sleep::Fading { since: t0 };
        let (n, step) = sleep_step(s, t0 + secs(5), true);
        assert_eq!(n, s);
        assert_eq!(step, SleepStep::Gain(0.25));
        let at = t0 + SLEEP_FADE;
        assert_eq!(
            sleep_step(s, at, true),
            (Sleep::Pausing { since: at }, SleepStep::Pause)
        );
    }

    #[test]
    fn sleep_fade_interrupted_by_pause_restores_after_delay() {
        let t0 = Instant::now();
        let t1 = t0 + secs(3);
        assert_eq!(
            sleep_step(Sleep::Fading { since: t0 }, t1, false),
            (Sleep::Pausing { since: t1 }, SleepStep::Idle)
        );
        let p = Sleep::Pausing { since: t1 };
        assert_eq!(sleep_step(p, t1 + Duration::from_millis(100), false), (p, SleepStep::Idle));
        assert_eq!(
            sleep_step(p, t1 + SLEEP_RESTORE_DELAY, false),
            (Sleep::Off, SleepStep::Restore)
        );
    }

    #[test]
    fn sleep_restore_waits_for_state_and_delay() {
        let t0 = Instant::now();
        let p = Sleep::Pausing { since: t0 };
        assert_eq!(sleep_step(p, t0 + Duration::from_millis(100), true), (p, SleepStep::Idle));
        assert_eq!(
            sleep_step(p, t0 + Duration::from_millis(200), false),
            (Sleep::Off, SleepStep::Restore)
        );
        assert_eq!(sleep_step(p, t0 + secs(2), true), (Sleep::Off, SleepStep::Restore));
    }

    #[test]
    fn fade_gain_curve() {
        assert_eq!(fade_gain(secs(0)), 1.0);
        assert_eq!(fade_gain(secs(5)), 0.25);
        assert_eq!(fade_gain(secs(10)), 0.0);
        assert_eq!(fade_gain(secs(20)), 0.0);
    }

    #[test]
    fn armed_label_formats() {
        let t0 = Instant::now();
        assert_eq!(armed_label(false, Sleep::Off, t0), None);
        assert_eq!(armed_label(true, Sleep::Off, t0).as_deref(), Some("STOP AFTER"));
        let a = Sleep::Armed { deadline: t0 + secs(30 * 60) };
        assert_eq!(
            armed_label(false, a, t0 + secs(1)).as_deref(),
            Some("SLEEP 30m")
        );
        let a = Sleep::Armed { deadline: t0 + Duration::from_millis(44_200) };
        assert_eq!(armed_label(false, a, t0).as_deref(), Some("SLEEP 45s"));
        let at = |d: Duration| Sleep::Armed { deadline: t0 + d };
        assert_eq!(
            armed_label(false, at(secs(29 * 60 + 30)), t0).as_deref(),
            Some("SLEEP 30m")
        );
        assert_eq!(
            armed_label(false, at(Duration::from_millis(59_400)), t0).as_deref(),
            Some("SLEEP 60s")
        );
        assert_eq!(
            armed_label(true, Sleep::Fading { since: t0 }, t0).as_deref(),
            Some("STOP AFTER SLEEP")
        );
    }
}
