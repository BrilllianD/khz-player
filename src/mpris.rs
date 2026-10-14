//! MPRIS D-Bus service on its own thread (mpris-server's Player is !Send).

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId};

use crate::audio::PlayerState;
use crate::playlist::Repeat;

/// Well-known bus name of the running player; also the single-instance lock.
pub const BUS_NAME: &str = "org.mpris.MediaPlayer2.khz-player";

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
    /// `file://` URL forwarded by a second `khz-player FILE` invocation.
    OpenUri(String),
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

/// Inverse of `file_url`: local `file://` URL (empty or `localhost` host) to a path.
pub fn path_from_file_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let b = rest.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(OsString::from_vec(out)))
}

/// Outcome of handing the command line to an already running instance.
#[derive(Debug, PartialEq, Eq)]
pub enum Forward {
    NotRunning,
    Forwarded,
    Failed(String),
}

/// If a player owns `BUS_NAME`, enqueue `args` there (or raise its window when empty).
pub fn forward_to_running(args: &[PathBuf]) -> Forward {
    use mpris_server::zbus::{Connection, fdo::DBusProxy};
    futures_lite::future::block_on(async {
        let Ok(conn) = Connection::session().await else {
            return Forward::NotRunning;
        };
        let Ok(name) = BUS_NAME.try_into() else {
            return Forward::NotRunning;
        };
        let Ok(bus) = DBusProxy::new(&conn).await else {
            return Forward::NotRunning;
        };
        if !bus.name_has_owner(name).await.unwrap_or(false) {
            return Forward::NotRunning;
        }
        let path = "/org/mpris/MediaPlayer2";
        let r: mpris_server::zbus::Result<()> = async {
            if args.is_empty() {
                conn.call_method(Some(BUS_NAME), path, Some("org.mpris.MediaPlayer2"), "Raise", &())
                    .await?;
            }
            for a in args {
                let abs = std::path::absolute(a).unwrap_or_else(|_| a.clone());
                conn.call_method(
                    Some(BUS_NAME),
                    path,
                    Some("org.mpris.MediaPlayer2.Player"),
                    "OpenUri",
                    &(file_url(&abs),),
                )
                .await?;
            }
            Ok(())
        }
        .await;
        match r {
            Ok(()) => Forward::Forwarded,
            Err(e) => Forward::Failed(e.to_string()),
        }
    })
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
        .supported_mime_types([
            "audio/mpeg",
            "audio/flac",
            "audio/x-flac",
            "audio/ogg",
            "audio/x-vorbis+ogg",
            "audio/mp4",
            "audio/aac",
            "audio/x-m4a",
            "audio/x-aiff",
            "audio/x-wav",
        ])
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
        let s = send.clone();
        player.connect_quit(move |_| s(MprisAction::Quit));
        let s = send;
        player.connect_open_uri(move |_, uri| s(MprisAction::OpenUri(uri.to_string())));
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
    fn file_url_round_trips() {
        for p in ["/m/a b/Кино #1%.mp3", "/a/%25 b.mp3"] {
            assert_eq!(path_from_file_url(&file_url(Path::new(p))), Some(PathBuf::from(p)));
        }
        let raw = Path::new(std::ffi::OsStr::from_bytes(b"/m/\xFF.mp3"));
        assert_eq!(path_from_file_url(&file_url(raw)).as_deref(), Some(raw));
    }

    #[test]
    fn path_from_localhost_url() {
        assert_eq!(
            path_from_file_url("file://localhost/a/b.mp3"),
            Some(PathBuf::from("/a/b.mp3"))
        );
    }

    #[test]
    fn path_from_url_rejects_remote_and_relative() {
        for u in ["http://x/a.mp3", "file://host/a.mp3", "file://a.mp3", "file:///a%2"] {
            assert_eq!(path_from_file_url(u), None, "{u}");
        }
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
