//! MPRIS D-Bus service on its own thread (mpris-server's Player is !Send).

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
        .can_play(true)
        .can_pause(true)
        .can_go_next(true)
        .can_go_previous(true)
        .can_seek(true)
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
        player.connect_set_position(move |_, _, pos| s(MprisAction::SetPosition(pos.as_micros())));
        let s = send.clone();
        player.connect_set_volume(move |_, v| s(MprisAction::Volume(v)));
        let s = send.clone();
        player.connect_set_shuffle(move |_, v| s(MprisAction::Shuffle(v)));
        let s = send.clone();
        player.connect_set_loop_status(move |_, l| {
            s(MprisAction::Repeat(match l {
                LoopStatus::None => Repeat::Off,
                LoopStatus::Playlist => Repeat::All,
                LoopStatus::Track => Repeat::One,
            }))
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
                    b = b.url(format!("file://{}", path.display()));
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
            };
            if let Err(e) = r {
                tracing::debug!("mpris update: {e}");
            }
        }
    };
    futures_lite::future::zip(player.run(), update_loop).await;
    Ok(())
}
