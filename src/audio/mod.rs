pub mod decoder;
pub mod dsp;
pub mod engine;
pub mod output;
pub mod resample;
pub mod shared;
pub mod spectrum;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

pub use decoder::TrackInfo;
use shared::Shared;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayerState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone)]
pub enum Command {
    Load { path: PathBuf, play: bool },
    Play,
    Pause,
    Stop,
    Seek(Duration),
    /// Relative seek in milliseconds.
    SeekRel(i64),
    /// Opens the next track ahead of time for gapless playback.
    PrefetchNext(Option<PathBuf>),
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// A track was loaded explicitly and is now current.
    Loaded(TrackInfo),
    /// Gapless transition: the prefetched track became audible.
    Advanced(TrackInfo),
    StateChanged(PlayerState),
    /// Current track finished and nothing was prefetched.
    TrackEnded,
    Seeked(Duration),
    Error { path: Option<PathBuf>, msg: String },
}

pub struct AudioHandle {
    pub tx: Sender<Command>,
    pub events: Receiver<Event>,
    pub shared: Arc<Shared>,
    pub tap: Option<rtrb::Consumer<f32>>,
    pub init_error: Option<String>,
    _output: Option<output::Output>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AudioHandle {
    pub fn start(volume: f32, balance: f32, repaint: egui::Context) -> Self {
        let shared = Arc::new(Shared::new(volume, balance));
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (ring, tap, out, init_error) = match output::open(shared.clone()) {
            Ok(p) => (p.ring, Some(p.tap), Some(p.output), None),
            Err(e) => {
                tracing::error!("audio output unavailable: {e:#}");
                // Keep the engine alive with a ring nobody reads so commands still work.
                let (ring, _) = rtrb::RingBuffer::new(2);
                (ring, None, None, Some(format!("{e:#}")))
            }
        };
        let eng_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("audio-engine".into())
            .spawn(move || engine::run(rx, ev_tx, eng_shared, ring, repaint))
            .expect("spawn audio engine");
        Self {
            tx,
            events: ev_rx,
            shared,
            tap,
            init_error,
            _output: out,
            thread: Some(thread),
        }
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(cmd);
    }

    pub fn position(&self) -> Duration {
        let rate = self.shared.out_rate().max(1) as f64;
        Duration::from_secs_f64(self.shared.position_frames() as f64 / rate)
    }
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
