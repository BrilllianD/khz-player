//! Pure part of the mpv wrapper: the event type the app sees, the state machine that maps
//! raw mpv events onto it, and value mapping for volume and output device. No libmpv here,
//! so everything is tested in both builds.
//!
//! `Seeked` follows every `Seek` + `PlaybackRestart` pair. `load` sets mpv's `start` only for
//! a nonzero start, so a plain load queues no seek; a load with a nonzero start does emit
//! `Seeked(start)`, accepted as an accurate position report.

use std::path::PathBuf;
use std::time::Duration;

use crate::audio::dsp::channel_gains;

/// What the app learns about the mpv window.
#[derive(Debug, Clone, PartialEq)]
pub enum VideoEvent {
    /// A file finished loading; the path is the one passed to `load`.
    Started(PathBuf),
    /// mpv's `pause` property changed (also from the mpv window).
    Paused(bool),
    Duration(Duration),
    /// A seek completed.
    Seeked(Duration),
    /// The file ended: `error: None` at its end, `Some(text)` when it failed.
    /// Never sent for `stop`, a newer `load` or quit.
    Ended { path: PathBuf, error: Option<String> },
    /// The mpv core shut down without the owner asking for it.
    WindowClosed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum EndReason {
    Eof,
    Stop,
    Quit,
    Error,
    Redirect,
}

/// Events as the mpv thread reads them, before mapping.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Raw {
    /// Path read by the thread from the `path` property.
    StartFile(Option<PathBuf>),
    FileLoaded,
    EndFile {
        reason: EndReason,
        error: Option<String>,
    },
    Pause(bool),
    TimePos(Option<f64>),
    Duration(Option<f64>),
    EofReached(bool),
    Seek,
    /// `time-pos` read by the thread at restart.
    PlaybackRestart(Option<f64>),
    Shutdown {
        ours: bool,
    },
}

pub(crate) struct Mapper {
    current: Option<PathBuf>,
    paused: bool,
    eof: bool,
    ended: bool,
    seeking: bool,
    duration: Option<Duration>,
    position: Duration,
}

impl Mapper {
    pub(crate) fn new() -> Self {
        Mapper {
            current: None,
            paused: false,
            eof: false,
            ended: false,
            seeking: false,
            duration: None,
            position: Duration::ZERO,
        }
    }

    pub(crate) fn position(&self) -> Duration {
        self.position
    }

    pub(crate) fn feed(&mut self, raw: Raw) -> Vec<VideoEvent> {
        let mut out = Vec::new();
        match raw {
            Raw::StartFile(p) => {
                if p.is_none() {
                    tracing::warn!("mpv start-file without a path");
                }
                self.current = p;
                self.eof = false;
                self.ended = false;
                self.seeking = false;
                self.duration = None;
                self.position = Duration::ZERO;
            }
            Raw::FileLoaded => {
                if let Some(p) = &self.current {
                    out.push(VideoEvent::Started(p.clone()));
                }
            }
            Raw::Duration(Some(s)) if s.is_finite() && s > 0.0 => {
                let d = Duration::from_secs_f64(s);
                if self.duration != Some(d) {
                    self.duration = Some(d);
                    out.push(VideoEvent::Duration(d));
                }
            }
            Raw::Duration(_) => {}
            Raw::TimePos(Some(s)) if s.is_finite() && s >= 0.0 => {
                self.position = Duration::from_secs_f64(s);
            }
            Raw::TimePos(_) => {}
            Raw::Pause(b) => {
                if b != self.paused {
                    self.paused = b;
                    if self.current.is_some() && !(b && self.eof) {
                        out.push(VideoEvent::Paused(b));
                    }
                }
            }
            Raw::EofReached(true) => {
                self.eof = true;
                self.end(None, &mut out);
            }
            Raw::EofReached(false) => {
                self.eof = false;
                self.ended = false;
            }
            Raw::EndFile { reason, error } => {
                match reason {
                    EndReason::Eof => {
                        self.eof = true;
                        self.end(None, &mut out);
                    }
                    EndReason::Error => {
                        let text = error.unwrap_or_else(|| "playback error".to_string());
                        self.end(Some(text), &mut out);
                    }
                    EndReason::Stop | EndReason::Quit | EndReason::Redirect => {}
                }
                self.current = None;
            }
            Raw::Seek => self.seeking = true,
            Raw::PlaybackRestart(t) => {
                if self.seeking {
                    self.seeking = false;
                    if let Some(s) = t.filter(|s| s.is_finite() && *s >= 0.0) {
                        self.position = Duration::from_secs_f64(s);
                    }
                    out.push(VideoEvent::Seeked(self.position));
                }
            }
            Raw::Shutdown { ours } => {
                if !ours {
                    out.push(VideoEvent::WindowClosed);
                }
            }
        }
        out
    }

    /// Emits `Ended` once per file; with no current path nothing is sent.
    fn end(&mut self, error: Option<String>, out: &mut Vec<VideoEvent>) {
        if self.ended {
            return;
        }
        self.ended = true;
        match &self.current {
            Some(path) => out.push(VideoEvent::Ended {
                path: path.clone(),
                error,
            }),
            None => tracing::debug!("mpv end without a current file"),
        }
    }
}

/// mpv's software volume is cubic (`(volume/100)^3`), so the cube root of the engine's
/// gain gives mpv a volume with the same loudness.
pub(crate) fn mpv_volume(v: f32) -> f64 {
    100.0 * (channel_gains(v, 0.0).0 as f64).cbrt()
}

/// Maps a cpal device id (`pulseaudio:<sink>`, `alsa:<dev>`) to an mpv `audio-device` name.
pub(crate) fn mpv_audio_device(dev: Option<&str>) -> String {
    match dev {
        Some(d) => {
            if let Some(x) = d.strip_prefix("pulseaudio:") {
                format!("pulse/{x}")
            } else if let Some(x) = d.strip_prefix("alsa:") {
                format!("alsa/{x}")
            } else {
                "auto".to_string()
            }
        }
        None => "auto".to_string(),
    }
}

pub(crate) fn mpv_secs(d: Duration) -> String {
    format!("{:.3}", d.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn started(m: &mut Mapper, path: &str) {
        m.feed(Raw::StartFile(Some(p(path))));
        m.feed(Raw::FileLoaded);
    }

    fn ended(path: &str, error: Option<&str>) -> VideoEvent {
        VideoEvent::Ended {
            path: p(path),
            error: error.map(String::from),
        }
    }

    #[test]
    fn start_and_loaded_give_started() {
        let mut m = Mapper::new();
        assert!(m.feed(Raw::StartFile(Some(p("/a.mp4")))).is_empty());
        assert_eq!(m.feed(Raw::FileLoaded), vec![VideoEvent::Started(p("/a.mp4"))]);
    }

    #[test]
    fn loaded_without_path_is_silent() {
        let mut m = Mapper::new();
        m.feed(Raw::StartFile(None));
        assert!(m.feed(Raw::FileLoaded).is_empty());
    }

    #[test]
    fn duplicate_duration_sent_once() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        assert_eq!(
            m.feed(Raw::Duration(Some(2.5))),
            vec![VideoEvent::Duration(Duration::from_millis(2500))]
        );
        assert!(m.feed(Raw::Duration(Some(2.5))).is_empty());
        assert!(m.feed(Raw::Duration(None)).is_empty());
        assert!(m.feed(Raw::Duration(Some(0.0))).is_empty());
        assert!(m.feed(Raw::Duration(Some(f64::NAN))).is_empty());
    }

    #[test]
    fn eof_reached_ends_once() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        assert_eq!(m.feed(Raw::EofReached(true)), vec![ended("/a", None)]);
        assert!(m.feed(Raw::EofReached(true)).is_empty());
        let r = m.feed(Raw::EndFile {
            reason: EndReason::Eof,
            error: None,
        });
        assert!(r.is_empty());
    }

    #[test]
    fn pause_after_eof_is_silent() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        m.feed(Raw::EofReached(true));
        assert!(m.feed(Raw::Pause(true)).is_empty());
    }

    #[test]
    fn pause_mirrors_property() {
        let mut m = Mapper::new();
        assert!(m.feed(Raw::Pause(false)).is_empty());
        started(&mut m, "/a");
        assert_eq!(m.feed(Raw::Pause(true)), vec![VideoEvent::Paused(true)]);
        assert!(m.feed(Raw::Pause(true)).is_empty());
        assert_eq!(m.feed(Raw::Pause(false)), vec![VideoEvent::Paused(false)]);
    }

    #[test]
    fn seek_back_from_end_reports_end_again() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        assert_eq!(m.feed(Raw::EofReached(true)).len(), 1);
        m.feed(Raw::EofReached(false));
        assert_eq!(m.feed(Raw::EofReached(true)), vec![ended("/a", None)]);
    }

    #[test]
    fn error_end_carries_text() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        let r = m.feed(Raw::EndFile {
            reason: EndReason::Error,
            error: Some("loading failed".into()),
        });
        assert_eq!(r, vec![ended("/a", Some("loading failed"))]);
    }

    #[test]
    fn error_end_without_text_has_default() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        let r = m.feed(Raw::EndFile {
            reason: EndReason::Error,
            error: None,
        });
        assert_eq!(r, vec![ended("/a", Some("playback error"))]);
    }

    #[test]
    fn stop_quit_redirect_send_no_end() {
        for reason in [EndReason::Stop, EndReason::Quit, EndReason::Redirect] {
            let mut m = Mapper::new();
            started(&mut m, "/a");
            let r = m.feed(Raw::EndFile {
                reason,
                error: None,
            });
            assert!(r.is_empty(), "{reason:?}");
        }
    }

    #[test]
    fn end_with_no_current_path_is_not_sent() {
        let mut m = Mapper::new();
        assert!(m.feed(Raw::EofReached(true)).is_empty());
        let mut m = Mapper::new();
        let r = m.feed(Raw::EndFile {
            reason: EndReason::Error,
            error: Some("x".into()),
        });
        assert!(r.is_empty());
    }

    #[test]
    fn seek_right_after_started_gives_seeked() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        assert!(m.feed(Raw::Seek).is_empty());
        assert_eq!(
            m.feed(Raw::PlaybackRestart(Some(2.0))),
            vec![VideoEvent::Seeked(Duration::from_secs(2))]
        );
    }

    #[test]
    fn restart_without_seek_after_load_is_silent() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        assert!(m.feed(Raw::PlaybackRestart(Some(0.0))).is_empty());
    }

    #[test]
    fn seek_then_restart_gives_seeked() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        m.feed(Raw::PlaybackRestart(Some(0.0)));
        m.feed(Raw::Seek);
        assert_eq!(
            m.feed(Raw::PlaybackRestart(Some(3.0))),
            vec![VideoEvent::Seeked(Duration::from_secs(3))]
        );
        assert_eq!(m.position(), Duration::from_secs(3));
    }

    #[test]
    fn restart_without_seek_is_silent() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        m.feed(Raw::PlaybackRestart(Some(0.0)));
        assert!(m.feed(Raw::PlaybackRestart(Some(3.0))).is_empty());
    }

    #[test]
    fn start_file_resets_position() {
        let mut m = Mapper::new();
        started(&mut m, "/a");
        m.feed(Raw::TimePos(Some(4.2)));
        assert_eq!(m.position(), Duration::from_millis(4200));
        m.feed(Raw::StartFile(Some(p("/b"))));
        assert_eq!(m.position(), Duration::ZERO);
    }

    #[test]
    fn shutdown_ours_or_not() {
        let mut m = Mapper::new();
        assert_eq!(
            m.feed(Raw::Shutdown { ours: false }),
            vec![VideoEvent::WindowClosed]
        );
        assert!(m.feed(Raw::Shutdown { ours: true }).is_empty());
    }

    #[test]
    fn volume_endpoints() {
        assert_eq!(mpv_volume(0.0), 0.0);
        assert_eq!(mpv_volume(f32::NAN), 0.0);
        assert!((mpv_volume(1.0) - 100.0).abs() < 1e-4);
    }

    #[test]
    fn volume_cubic_matches_engine_gain() {
        for v in [0.1f32, 0.5, 0.9] {
            let g = ((mpv_volume(v) / 100.0).powi(3)) as f32;
            assert!((g - channel_gains(v, 0.0).0).abs() < 1e-5, "{v}");
        }
    }

    #[test]
    fn device_mapping() {
        assert_eq!(mpv_audio_device(None), "auto");
        assert_eq!(
            mpv_audio_device(Some("pulseaudio:alsa_output.x")),
            "pulse/alsa_output.x"
        );
        assert_eq!(mpv_audio_device(Some("alsa:hw:0,0")), "alsa/hw:0,0");
        assert_eq!(mpv_audio_device(Some("jack:x")), "auto");
    }

    #[test]
    fn secs_format() {
        assert_eq!(mpv_secs(Duration::from_millis(1500)), "1.500");
    }
}
