//! `VideoPlayer` over libmpv: one mpv core with its own window, one event thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::anyhow;
use crossbeam_channel::{Receiver, Sender};
use libmpv2::events::{Event, PropertyData};
use libmpv2::{Format, Mpv, mpv_end_file_reason, mpv_error};

use super::events::{EndReason, Mapper, Raw, VideoEvent, mpv_audio_device, mpv_secs, mpv_volume};

const WAIT_SECS: f64 = 0.5;

pub struct VideoPlayer {
    mpv: Arc<Mpv>,
    rx: Receiver<VideoEvent>,
    /// For events made on the caller's side (a path mpv cannot be given).
    tx: Sender<VideoEvent>,
    pos_ms: Arc<AtomicU64>,
    alive: Arc<AtomicBool>,
    closing: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl VideoPlayer {
    /// Creates the mpv core and its event thread. The window appears with the first file.
    pub fn new(repaint: egui::Context) -> anyhow::Result<Self> {
        Self::with_options(repaint, &[])
    }

    /// `extra` options are applied after the defaults (tests use them for a null output).
    fn with_options(repaint: egui::Context, extra: &[(&str, &str)]) -> anyhow::Result<Self> {
        let mpv = Mpv::with_initializer(|init| {
            for (k, v) in [
                ("ytdl", "no"),
                ("idle", "yes"),
                ("keep-open", "yes"),
                ("force-window", "yes"),
                ("input-default-bindings", "yes"),
                ("input-vo-keyboard", "yes"),
            ] {
                init.set_option(k, v)?;
            }
            for (k, v) in [
                ("wayland-app-id", "khz-player-video"),
                ("title", "${media-title} - khz-player"),
                ("audio-client-name", "khz-player"),
            ] {
                if let Err(e) = init.set_option(k, v) {
                    tracing::warn!("mpv option {k}: {}", error_text(&e));
                }
            }
            for &(k, v) in extra {
                init.set_option(k, v)?;
            }
            Ok(())
        })
        .map_err(|e| anyhow!("libmpv: {}", error_text(&e)))?;
        for (name, format, id) in [
            ("pause", Format::Flag, 1),
            ("time-pos", Format::Double, 2),
            ("duration", Format::Double, 3),
            ("eof-reached", Format::Flag, 4),
        ] {
            mpv.observe_property(name, format, id)
                .map_err(|e| anyhow!("libmpv: observe {name}: {}", error_text(&e)))?;
        }
        let mpv = Arc::new(mpv);
        let (tx, rx) = crossbeam_channel::unbounded();
        let pos_ms = Arc::new(AtomicU64::new(0));
        let alive = Arc::new(AtomicBool::new(true));
        let closing = Arc::new(AtomicBool::new(false));
        let thread = {
            let (mpv, tx, pos_ms) = (mpv.clone(), tx.clone(), pos_ms.clone());
            let (alive, closing) = (alive.clone(), closing.clone());
            std::thread::Builder::new()
                .name("mpv-events".into())
                .spawn(move || event_loop(&mpv, &tx, &pos_ms, &alive, &closing, &repaint))?
        };
        Ok(VideoPlayer {
            mpv,
            rx,
            tx,
            pos_ms,
            alive,
            closing,
            thread: Some(thread),
        })
    }

    fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn log<T>(what: &str, r: libmpv2::Result<T>) {
        if let Err(e) = r {
            tracing::debug!("mpv {what}: {}", error_text(&e));
        }
    }

    /// Loads `path` replacing the current file; `start` and `pause` are set as properties
    /// because the argument position of `loadfile` per-file options differs between mpv versions.
    pub fn load(&self, path: &Path, play: bool, start: Duration) {
        if !self.alive() {
            return;
        }
        let Some(p) = path.to_str() else {
            let _ = self.tx.send(VideoEvent::Ended {
                path: path.to_path_buf(),
                error: Some("path is not valid UTF-8".into()),
            });
            return;
        };
        Self::log("pause", self.mpv.set_property("pause", !play));
        // A set `start` makes mpv queue a start seek, even "0.000"; "none" avoids it.
        let start = if start == Duration::ZERO {
            "none".to_string()
        } else {
            mpv_secs(start)
        };
        Self::log("start", self.mpv.set_property("start", start.as_str()));
        Self::log("loadfile", self.mpv.command("loadfile", &[p, "replace"]));
    }

    pub fn play(&self) {
        if self.alive() {
            Self::log("play", self.mpv.set_property("pause", false));
        }
    }

    pub fn pause(&self) {
        if self.alive() {
            Self::log("pause", self.mpv.set_property("pause", true));
        }
    }

    pub fn stop(&self) {
        if self.alive() {
            Self::log("stop", self.mpv.command("stop", &[]));
        }
    }

    pub fn seek(&self, to: Duration) {
        if self.alive() {
            Self::log(
                "seek",
                self.mpv.command("seek", &[&mpv_secs(to), "absolute"]),
            );
        }
    }

    pub fn seek_rel(&self, ms: i64) {
        if self.alive() {
            let s = format!("{:.3}", ms as f64 / 1000.0);
            Self::log("seek", self.mpv.command("seek", &[&s, "relative"]));
        }
    }

    pub fn set_volume(&self, v: f32) {
        if self.alive() {
            Self::log("volume", self.mpv.set_property("volume", mpv_volume(v)));
        }
    }

    pub fn set_device(&self, dev: Option<&str>) {
        if self.alive() {
            Self::log(
                "audio-device",
                self.mpv
                    .set_property("audio-device", mpv_audio_device(dev).as_str()),
            );
        }
    }

    /// Best effort: toggling `ontop` raises the window on X11; on Wayland the compositor decides.
    pub fn raise(&self) {
        if self.alive() {
            Self::log("ontop", self.mpv.set_property("ontop", true));
            Self::log("ontop", self.mpv.set_property("ontop", false));
        }
    }

    pub fn position(&self) -> Duration {
        Duration::from_millis(self.pos_ms.load(Ordering::Relaxed))
    }

    pub fn poll(&self) -> Vec<VideoEvent> {
        self.rx.try_iter().collect()
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Release);
        if self.alive() {
            Self::log("quit", self.mpv.command("quit", &[]));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn event_loop(
    mpv: &Mpv,
    tx: &Sender<VideoEvent>,
    pos_ms: &AtomicU64,
    alive: &AtomicBool,
    closing: &AtomicBool,
    repaint: &egui::Context,
) {
    let mut mapper = Mapper::new();
    loop {
        let (raw, last) = match mpv.wait_event(WAIT_SECS) {
            None => {
                if closing.load(Ordering::Acquire) {
                    break;
                }
                continue;
            }
            Some(Err(e)) => (
                Some(Raw::EndFile {
                    reason: EndReason::Error,
                    error: Some(error_text(&e)),
                }),
                false,
            ),
            Some(Ok(ev)) => match ev {
                Event::StartFile => (
                    Some(Raw::StartFile(
                        mpv.get_property::<String>("path").ok().map(PathBuf::from),
                    )),
                    false,
                ),
                Event::FileLoaded => (Some(Raw::FileLoaded), false),
                Event::EndFile(r) => (
                    Some(Raw::EndFile {
                        reason: end_reason(r),
                        error: None,
                    }),
                    false,
                ),
                Event::Seek => (Some(Raw::Seek), false),
                Event::PlaybackRestart => (
                    Some(Raw::PlaybackRestart(mpv.get_property::<f64>("time-pos").ok())),
                    false,
                ),
                Event::PropertyChange { name, change, .. } => (property(name, change), false),
                Event::Shutdown => (
                    Some(Raw::Shutdown {
                        ours: closing.load(Ordering::Acquire),
                    }),
                    true,
                ),
                _ => (None, false),
            },
        };
        if let Some(raw) = raw {
            let events = mapper.feed(raw);
            pos_ms.store(mapper.position().as_millis() as u64, Ordering::Relaxed);
            if !events.is_empty() {
                for e in events {
                    let _ = tx.send(e);
                }
                repaint.request_repaint();
            }
        }
        if last {
            alive.store(false, Ordering::Release);
            break;
        }
    }
}

fn property(name: &str, change: PropertyData<'_>) -> Option<Raw> {
    match (name, change) {
        ("pause", PropertyData::Flag(b)) => Some(Raw::Pause(b)),
        ("eof-reached", PropertyData::Flag(b)) => Some(Raw::EofReached(b)),
        ("time-pos", PropertyData::Double(s)) => Some(Raw::TimePos(Some(s))),
        ("time-pos", PropertyData::None) => Some(Raw::TimePos(None)),
        ("duration", PropertyData::Double(s)) => Some(Raw::Duration(Some(s))),
        ("duration", PropertyData::None) => Some(Raw::Duration(None)),
        _ => None,
    }
}

fn end_reason(r: libmpv2::EndFileReason) -> EndReason {
    match r {
        mpv_end_file_reason::Eof => EndReason::Eof,
        mpv_end_file_reason::Stop => EndReason::Stop,
        mpv_end_file_reason::Quit => EndReason::Quit,
        mpv_end_file_reason::Error => EndReason::Error,
        mpv_end_file_reason::Redirect => EndReason::Redirect,
        _ => EndReason::Stop,
    }
}

/// `libmpv2::Error` prints as `Debug`; this gives short texts for the codes users hit.
fn error_text(e: &libmpv2::Error) -> String {
    match e {
        libmpv2::Error::Raw(code) => match *code {
            mpv_error::LoadingFailed => "loading failed".into(),
            mpv_error::UnknownFormat => "unknown file format".into(),
            mpv_error::NothingToPlay => "nothing to play".into(),
            mpv_error::AoInitFailed => "audio output failed".into(),
            mpv_error::VoInitFailed => "video output failed".into(),
            c => format!("mpv error {c}"),
        },
        libmpv2::Error::VersionMismatch { loaded, .. } => {
            format!("libmpv API {}.{}, need 2.x", loaded >> 16, loaded & 0xffff)
        }
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decoder::fixtures::ffmpeg_fixture;
    use std::time::Instant;

    const NULL_OUT: &[(&str, &str)] = &[("vo", "null"), ("ao", "null"), ("force-window", "no")];

    fn player() -> VideoPlayer {
        VideoPlayer::with_options(egui::Context::default(), NULL_OUT).expect("libmpv new")
    }

    fn fixture(name: &str) -> Option<PathBuf> {
        ffmpeg_fixture(
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=64x48:rate=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
                "-shortest",
            ],
            name,
        )
    }

    /// Polls until `pick` returns a value from an event, or fails after 10 s with the events seen.
    fn wait_for<T>(
        vp: &VideoPlayer,
        seen: &mut Vec<VideoEvent>,
        what: &str,
        mut pick: impl FnMut(&VideoEvent) -> Option<T>,
    ) -> T {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            for e in vp.poll() {
                seen.push(e);
            }
            if let Some(i) = seen.iter().position(|e| pick(e).is_some()) {
                let e = seen.remove(i);
                return pick(&e).unwrap();
            }
            assert!(Instant::now() < deadline, "timeout waiting for {what}; seen {seen:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn plays_generated_file_to_end() {
        let Some(p) = fixture("video-mpv-1s.mp4") else { return };
        let vp = player();
        vp.load(&p, true, Duration::ZERO);
        let mut seen = Vec::new();
        wait_for(&vp, &mut seen, "Started", |e| {
            (e == &VideoEvent::Started(p.clone())).then_some(())
        });
        let d = wait_for(&vp, &mut seen, "Duration", |e| match e {
            VideoEvent::Duration(d) => Some(*d),
            _ => None,
        });
        assert!((0.8..=1.3).contains(&d.as_secs_f64()), "duration {d:?}");
        wait_for(&vp, &mut seen, "Ended", |e| {
            (e == &VideoEvent::Ended {
                path: p.clone(),
                error: None,
            })
            .then_some(())
        });
        assert!(
            !seen.iter().any(|e| matches!(e, VideoEvent::Seeked(_))),
            "spurious Seeked after a plain load: {seen:?}"
        );
    }

    #[test]
    fn seek_reports_position() {
        let Some(p) = fixture("video-mpv-seek.mp4") else { return };
        let vp = player();
        vp.load(&p, false, Duration::ZERO);
        let mut seen = Vec::new();
        wait_for(&vp, &mut seen, "Started", |e| {
            matches!(e, VideoEvent::Started(_)).then_some(())
        });
        vp.seek(Duration::from_millis(500));
        let d = wait_for(&vp, &mut seen, "Seeked", |e| match e {
            VideoEvent::Seeked(d) => Some(*d),
            _ => None,
        });
        assert!((0.35..=0.65).contains(&d.as_secs_f64()), "seeked {d:?}");
        let pos = vp.position().as_secs_f64();
        assert!((0.35..=0.65).contains(&pos), "position {pos}");
    }

    #[test]
    fn missing_file_ends_with_error() {
        let vp = player();
        let p = PathBuf::from("/nonexistent/khz-video.mp4");
        vp.load(&p, true, Duration::ZERO);
        let mut seen = Vec::new();
        wait_for(&vp, &mut seen, "Ended with error", |e| match e {
            VideoEvent::Ended { path, error: Some(_) } if *path == p => Some(()),
            _ => None,
        });
    }

    #[test]
    fn drop_does_not_hang() {
        let Some(p) = fixture("video-mpv-drop.mp4") else { return };
        let vp = player();
        vp.load(&p, true, Duration::ZERO);
        drop(vp);
    }
}
