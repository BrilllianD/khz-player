pub mod decoder;
pub mod dsp;
pub mod engine;
pub mod output;
pub mod resample;
pub mod shared;
pub mod spectrum;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
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

#[derive(Debug)]
pub enum Command {
    Load { path: PathBuf, play: bool },
    /// Loads `path` paused at `at`; restores the previous session.
    Resume { path: PathBuf, at: Duration },
    Play,
    Pause,
    Stop,
    Seek(Duration),
    /// Relative seek in milliseconds.
    SeekRel(i64),
    /// Opens the next track ahead of time for gapless playback.
    PrefetchNext(Option<PathBuf>),
    /// The output was reopened: feed `ring` at `rate` from now on.
    SetOutput { ring: rtrb::Producer<f32>, rate: u32 },
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
    /// `Load`/`Resume` could not open the file; `play` is whether playback
    /// was requested.
    LoadFailed { path: PathBuf, msg: String, play: bool },
    Error { path: Option<PathBuf>, msg: String },
}

pub struct AudioHandle {
    pub tx: Sender<Command>,
    pub events: Receiver<Event>,
    pub shared: Arc<Shared>,
    pub tap: Option<rtrb::Consumer<f32>>,
    pub init_error: Option<String>,
    _output: Option<output::Output>,
    repaint: egui::Context,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AudioHandle {
    pub fn start(volume: f32, balance: f32, repaint: egui::Context) -> Self {
        let shared = Arc::new(Shared::new(volume, balance));
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (ring, tap, out, init_error) = match output::open(shared.clone(), repaint.clone()) {
            Ok(p) => {
                shared.out_rate.store(p.rate, Ordering::Relaxed);
                (p.ring, Some(p.tap), Some(p.output), None)
            }
            Err(e) => {
                tracing::error!("audio output unavailable: {e:#}");
                // Keep the engine alive with a ring nobody reads so commands
                // still work; flushes must not wait for a callback that never runs.
                shared.device_lost.store(true, Ordering::Release);
                let (ring, _) = rtrb::RingBuffer::new(2);
                (ring, None, None, Some(format!("{e:#}")))
            }
        };
        let (eng_shared, eng_repaint) = (shared.clone(), repaint.clone());
        let thread = std::thread::Builder::new()
            .name("audio-engine".into())
            .spawn(move || engine::run(rx, ev_tx, eng_shared, ring, eng_repaint))
            .expect("spawn audio engine");
        Self {
            tx,
            events: ev_rx,
            shared,
            tap,
            init_error,
            _output: out,
            repaint,
            thread: Some(thread),
        }
    }

    /// Opens the default output again after a device loss or a failed start
    /// and hands the new ring to the engine. Returns the new output rate.
    pub fn reopen(&mut self) -> anyhow::Result<u32> {
        // Close the old stream first: a device that is still there may not
        // allow a second stream, and its callback must stop counting `consumed`
        // before the engine resyncs to the new ring.
        self._output = None;
        self.tap = None;
        // Cleared before the new stream starts so an error it reports right
        // away is not lost.
        self.shared.device_lost.store(false, Ordering::Release);
        let p = match output::open(self.shared.clone(), self.repaint.clone()) {
            Ok(p) => p,
            Err(e) => {
                self.shared.device_lost.store(true, Ordering::Release);
                return Err(e);
            }
        };
        self._output = Some(p.output);
        self.tap = Some(p.tap);
        self.init_error = None;
        self.send(Command::SetOutput {
            ring: p.ring,
            rate: p.rate,
        });
        Ok(p.rate)
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
