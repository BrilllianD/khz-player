//! MPRIS D-Bus service on its own thread (mpris-server's Player is !Send).

use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId};

use crate::audio::PlayerState;
use crate::playlist::Repeat;

#[derive(Debug, Clone)]
pub enum MprisAction {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    /// Relative seek.
    Seek(i64),
    /// Absolute position in microseconds.
    SetPosition(i64),
    Volume(f64),
    Shuffle(bool),
    Repeat(Repeat),
    Raise,
    Quit,
}

#[derive(Debug, Clone)]
pub enum MprisUpdate {
    Status(PlayerState),
    Track {
        id: u64,
        title: String,
        artist: Option<String>,
        album: Option<String>,
        length: Option<Duration>,
        path: std::path::PathBuf,
    },
    Position(Duration),
    Seeked(Duration),
    Volume(f64),
    Shuffle(bool),
    Repeat(Repeat),
    Caps(Caps),
}

/// What the app can currently do; drives the MPRIS Can* properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Caps {
    pub play: bool,
    pub seek: bool,
    pub next: bool,
    pub prev: bool,
}

pub struct Mpris {
    pub updates: async_channel::Sender<MprisUpdate>,
    pub actions: Receiver<MprisAction>,
}

impl Mpris {
    pub fn send(&self, u: MprisUpdate) {
        let _ = self.updates.try_send(u);
    }
}

fn time(d: Duration) -> Time {
    Time::from_micros(d.as_micros() as i64)
}

fn loop_status(r: Repeat) -> LoopStatus {
    match r {
        Repeat::Off => LoopStatus::None,
        Repeat::All => LoopStatus::Playlist,
        Repeat::One => LoopStatus::Track,
    }
}

fn repeat_from(l: LoopStatus) -> Repeat {
    match l {
        LoopStatus::None => Repeat::Off,
        LoopStatus::Playlist => Repeat::All,
        LoopStatus::Track => Repeat::One,
    }
}

/// Percent-encoded `file://` URL; every byte outside the unreserved set and `/` is escaped.
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

fn set_position_ok(current: Option<&TrackId>, len: Option<Time>, req: &TrackId, pos: Time) -> bool {
    current == Some(req) && pos >= Time::ZERO && len.is_none_or(|l| pos <= l)
}

/// True when a relative MPRIS seek of `offset_us` from `pos` reaches the end of the track.
pub fn seek_past_end(pos: Duration, offset_us: i64, len: Option<Duration>) -> bool {
    let Some(l) = len else { return false };
    offset_us > 0 && pos + Duration::from_micros(offset_us as u64) >= l
}

fn track_id(id: u64) -> TrackId {
    TrackId::try_from(format!("/org/khz_player/track/{id}")).unwrap_or(TrackId::NO_TRACK)
}

pub fn start(repaint: egui::Context) -> Mpris {
    let (up_tx, up_rx) = async_channel::unbounded::<MprisUpdate>();
    let (act_tx, act_rx) = crossbeam_channel::unbounded::<MprisAction>();
    let spawned = std::thread::Builder::new()
        .name("mpris".into())
        .spawn(move || {
            if let Err(e) = futures_lite::future::block_on(serve(up_rx, act_tx, repaint)) {
                tracing::warn!("MPRIS unavailable: {e}");
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("spawn mpris: {e}");
    }
    Mpris {
        updates: up_tx,
        actions: act_rx,
    }
}

async fn serve(
    updates: async_channel::Receiver<MprisUpdate>,
    actions: Sender<MprisAction>,
    repaint: egui::Context,
) -> mpris_server::zbus::Result<()> {
    let player = Player::builder("khz-player")
        .identity("khz-player")
        .desktop_entry("khz-player")
        .can_play(false)
        .can_pause(false)
        .can_go_next(false)
        .can_go_previous(false)
        .can_seek(false)
        .can_raise(true)
        .can_quit(true)
        .supported_uri_schemes(["file"])
        .supported_mime_types(["audio/mpeg", "audio/flac", "audio/ogg", "audio/mp4", "audio/x-wav"])
        .build()
        .await?;

    let send = move |a: MprisAction| {
        let _ = actions.send(a);
        repaint.request_repaint();
    };
    {
        let s = send.clone();
        player.connect_play_pause(move |_| s(MprisAction::PlayPause));
        let s = send.clone();
        player.connect_play(move |_| s(MprisAction::Play));
        let s = send.clone();
        player.connect_pause(move |_| s(MprisAction::Pause));
        let s = send.clone();
        player.connect_stop(move |_| s(MprisAction::Stop));
        let s = send.clone();
        player.connect_next(move |_| s(MprisAction::Next));
        let s = send.clone();
        player.connect_previous(move |_| s(MprisAction::Previous));
        let s = send.clone();
        player.connect_seek(move |_, offset| s(MprisAction::Seek(offset.as_micros())));
        let s = send.clone();
        player.connect_set_position(move |p, id, pos| {
            let ok = {
                let m = p.metadata();
                set_position_ok(m.trackid().as_ref(), m.length(), id, pos)
            };
            if ok {
                s(MprisAction::SetPosition(pos.as_micros()));
            }
        });
        let s = send.clone();
        player.connect_set_volume(move |_, v| s(MprisAction::Volume(v)));
        let s = send.clone();
        player.connect_set_shuffle(move |_, v| s(MprisAction::Shuffle(v)));
        let s = send.clone();
        player.connect_set_loop_status(move |_, l| {
            s(MprisAction::Repeat(repeat_from(l)))
        });
        let s = send.clone();
        player.connect_raise(move |_| s(MprisAction::Raise));
        let s = send;
        player.connect_quit(move |_| s(MprisAction::Quit));
    }

    let update_loop = async {
        while let Ok(u) = updates.recv().await {
            let r = match u {
                MprisUpdate::Status(s) => {
                    player
                        .set_playback_status(match s {
                            PlayerState::Playing => PlaybackStatus::Playing,
                            PlayerState::Paused => PlaybackStatus::Paused,
                            PlayerState::Stopped => PlaybackStatus::Stopped,
                        })
                        .await
                }
                MprisUpdate::Track {
                    id,
                    title,
                    artist,
                    album,
                    length,
                    path,
                } => {
                    let mut b = Metadata::builder().trackid(track_id(id)).title(title);
                    if let Some(a) = artist {
                        b = b.artist([a]);
                    }
                    if let Some(a) = album {
                        b = b.album(a);
                    }
                    if let Some(l) = length {
                        b = b.length(time(l));
                    }
                    b = b.url(file_url(&path));
                    player.set_position(Time::ZERO);
                    player.set_metadata(b.build()).await
                }
                MprisUpdate::Position(p) => {
                    player.set_position(time(p));
                    Ok(())
                }
                MprisUpdate::Seeked(p) => {
                    player.set_position(time(p));
                    player.seeked(time(p)).await
                }
                MprisUpdate::Volume(v) => player.set_volume(v).await,
                MprisUpdate::Shuffle(v) => player.set_shuffle(v).await,
                MprisUpdate::Repeat(r) => player.set_loop_status(loop_status(r)).await,
                MprisUpdate::Caps(c) => {
                    async {
                        player.set_can_play(c.play).await?;
                        player.set_can_pause(c.play).await?;
                        player.set_can_seek(c.seek).await?;
                        player.set_can_go_next(c.next).await?;
                        player.set_can_go_previous(c.prev).await
                    }
                    .await
                }
            };
            if let Err(e) = r {
                tracing::debug!("mpris update: {e}");
            }
        }
    };
    futures_lite::future::zip(player.run(), update_loop).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tid(n: u64) -> TrackId {
        track_id(n)
    }

    #[test]
    fn file_url_percent_encodes() {
        assert_eq!(
            file_url(Path::new("/m/a b/Кино #1%.mp3")),
            "file:///m/a%20b/%D0%9A%D0%B8%D0%BD%D0%BE%20%231%25.mp3"
        );
    }

    #[test]
    fn file_url_encodes_non_utf8_bytes() {
        let p = Path::new(std::ffi::OsStr::from_bytes(b"/m/\xFF.mp3"));
        assert_eq!(file_url(p), "file:///m/%FF.mp3");
    }

    #[test]
    fn repeat_round_trips_through_loop_status() {
        assert_eq!(loop_status(Repeat::Off), LoopStatus::None);
        assert_eq!(loop_status(Repeat::All), LoopStatus::Playlist);
        assert_eq!(loop_status(Repeat::One), LoopStatus::Track);
        for r in [Repeat::Off, Repeat::All, Repeat::One] {
            assert_eq!(repeat_from(loop_status(r)), r);
        }
    }

    #[test]
    fn set_position_checks_id_and_range() {
        let cur = tid(1);
        let len = Some(Time::from_secs(10));
        let ok = |req: &TrackId, cur: Option<&TrackId>, pos: i64| {
            set_position_ok(cur, len, req, Time::from_micros(pos))
        };
        assert!(ok(&cur, Some(&cur), 5_000_000));
        assert!(ok(&cur, Some(&cur), 0));
        assert!(ok(&cur, Some(&cur), 10_000_000));
        assert!(!ok(&tid(2), Some(&cur), 5_000_000));
        assert!(!ok(&cur, Some(&cur), -1));
        assert!(!ok(&cur, Some(&cur), 10_000_001));
        assert!(!ok(&cur, None, 5_000_000));
    }

    #[test]
    fn seek_past_end_cases() {
        let s = Duration::from_secs;
        let len = Some(s(10));
        assert!(seek_past_end(s(8), 2_000_000, len));
        assert!(seek_past_end(s(8), 5_000_000, len));
        assert!(!seek_past_end(s(8), 1_000_000, len));
        assert!(!seek_past_end(s(8), -5_000_000, len));
        assert!(!seek_past_end(s(8), 5_000_000, None));
    }
}
