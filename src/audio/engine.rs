//! Decode thread: owns decoders, resamples to the output rate and feeds the ring.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, select};

use crate::audio::decoder::{Decoder, TrackInfo};
use crate::audio::resample::Resampler;
use crate::audio::shared::Shared;
use crate::audio::{Command, Event, PlayerState};

const IDLE_WAIT: Duration = Duration::from_millis(5);
const FLUSH_TIMEOUT: Duration = Duration::from_millis(100);
const XRUN_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// Result of a background open: (generation, path, decoder).
type Prefetched = (u64, PathBuf, anyhow::Result<Decoder>);

struct Current {
    decoder: Decoder,
    resampler: Resampler,
}

/// Replaces the resampler with one for the decoder's current rate and
/// `out_rate`. Buffered input of the old one is dropped; call `finish` first
/// to keep it.
fn rebuild_resampler(cur: &mut Current, out_rate: u32) -> anyhow::Result<()> {
    cur.resampler = Resampler::new(cur.decoder.info.sample_rate, out_rate)?;
    Ok(())
}

struct Engine {
    events: Sender<Event>,
    shared: Arc<Shared>,
    ring: rtrb::Producer<f32>,
    repaint: egui::Context,
    out_rate: u32,
    state: PlayerState,
    cur: Option<Current>,
    /// Path of the audible track, so Play after Stop restarts it.
    last_path: Option<PathBuf>,
    next: Option<(PathBuf, Decoder)>,
    /// Background opens report here; results whose generation is not
    /// `prefetch_gen` were superseded and are dropped.
    prefetch_tx: Sender<Prefetched>,
    prefetch_rx: Receiver<Prefetched>,
    prefetch_gen: u64,
    /// Open in flight for the current generation.
    pending: Option<PathBuf>,
    /// Decoded, resampled frames not yet pushed to the ring.
    buf: Vec<f32>,
    buf_pos: usize,
    scratch: Vec<f32>,
    /// Frames pushed to the ring, in the callback's `consumed` domain.
    pushed_total: i64,
    /// Gapless transitions waiting to become audible: (consumed mark, info).
    transitions: VecDeque<(i64, TrackInfo)>,
    /// Current decoder hit end of stream and nothing follows.
    eof: bool,
    /// Xrun count at the last log line, and when it was written.
    last_xruns: u64,
    last_xrun_log: Instant,
}

pub fn run(
    rx: Receiver<Command>,
    events: Sender<Event>,
    shared: Arc<Shared>,
    ring: rtrb::Producer<f32>,
    repaint: egui::Context,
) {
    let out_rate = shared.out_rate();
    let (prefetch_tx, prefetch_rx) = crossbeam_channel::unbounded();
    let mut e = Engine {
        events,
        shared,
        ring,
        repaint,
        out_rate,
        state: PlayerState::Stopped,
        cur: None,
        last_path: None,
        next: None,
        prefetch_tx,
        prefetch_rx,
        prefetch_gen: 0,
        pending: None,
        buf: Vec::with_capacity(16384),
        buf_pos: 0,
        scratch: Vec::with_capacity(16384),
        pushed_total: 0,
        transitions: VecDeque::new(),
        eof: false,
        last_xruns: 0,
        last_xrun_log: Instant::now(),
    };
    // The engine keeps a sender, so this never disconnects.
    let prefetched = e.prefetch_rx.clone();
    loop {
        // Only playback (or a gapless mark still to cross) needs ticks; otherwise
        // nothing changes until a command or a finished prefetch arrives.
        let wake = if e.state == PlayerState::Playing || !e.transitions.is_empty() {
            select! {
                recv(rx) -> c => Wake::Command(c.ok()),
                recv(prefetched) -> r => Wake::Prefetched(r.ok()),
                default(IDLE_WAIT) => Wake::Tick,
            }
        } else {
            select! {
                recv(rx) -> c => Wake::Command(c.ok()),
                recv(prefetched) -> r => Wake::Prefetched(r.ok()),
            }
        };
        match wake {
            Wake::Command(None) => break,
            Wake::Command(Some(c)) => {
                if !e.handle(c) {
                    break;
                }
                while let Ok(c) = rx.try_recv() {
                    if !e.handle(c) {
                        return;
                    }
                }
            }
            Wake::Prefetched(r) => {
                if let Some(r) = r {
                    e.prefetched(r);
                }
            }
            Wake::Tick => {}
        }
        if e.state == PlayerState::Playing {
            e.fill();
        }
        e.check_progress();
    }
}

enum Wake {
    /// `None` when the command channel disconnected.
    Command(Option<Command>),
    Prefetched(Option<Prefetched>),
    Tick,
}

impl Engine {
    fn emit(&self, ev: Event) {
        let _ = self.events.send(ev);
        self.repaint.request_repaint();
    }

    fn set_state(&mut self, s: PlayerState) {
        if self.state != s {
            self.state = s;
            self.shared
                .paused
                .store(s != PlayerState::Playing, Ordering::Relaxed);
            self.emit(Event::StateChanged(s));
        }
    }

    fn consumed(&self) -> i64 {
        self.shared.consumed.load(Ordering::Acquire) as i64
    }

    /// Discards everything buffered in the ring and waits for the callback to
    /// do it. Pending transitions are dropped: callers that keep the current
    /// decoder must announce them first. With the device lost no callback
    /// runs, so nothing waits; the next stream acks the flag on its first call.
    fn flush(&mut self) {
        self.buf.clear();
        self.buf_pos = 0;
        self.shared.flush.store(true, Ordering::Release);
        if !self.shared.device_lost.load(Ordering::Acquire) {
            let start = Instant::now();
            while self.shared.flush.load(Ordering::Acquire) && start.elapsed() < FLUSH_TIMEOUT {
                std::thread::sleep(Duration::from_millis(1));
            }
            if self.shared.flush.load(Ordering::Acquire) {
                tracing::debug!("flush not acknowledged by audio callback");
            }
        }
        self.pushed_total = self.consumed();
        self.transitions.clear();
    }

    fn advanced(&mut self, info: TrackInfo) {
        self.last_path = Some(info.path.clone());
        self.emit(Event::Advanced(info));
    }

    /// Returns false on shutdown.
    fn handle(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Load { path, play } => self.load(path, play),
            Command::Resume { path, at } => {
                self.load(path, false);
                if self.cur.is_some() {
                    if !at.is_zero() {
                        self.seek(at);
                    }
                    self.set_state(PlayerState::Paused);
                }
            }
            Command::Play => match self.state {
                PlayerState::Paused => self.set_state(PlayerState::Playing),
                PlayerState::Stopped => {
                    if self.cur.is_some() {
                        self.set_state(PlayerState::Playing);
                    } else if let Some(p) = self.last_path.clone() {
                        self.load(p, true);
                    }
                }
                PlayerState::Playing => {
                    // Winamp restarts the track on Play while playing.
                    self.seek(Duration::ZERO);
                }
            },
            Command::Pause => {
                if self.state == PlayerState::Playing {
                    self.set_state(PlayerState::Paused);
                }
            }
            Command::Stop => self.stop(),
            Command::Seek(pos) => self.seek(pos),
            Command::SeekRel(ms) => {
                if self.cur.is_some() {
                    let pos = self.position().as_millis() as i64 + ms;
                    let mut pos = Duration::from_millis(pos.max(0) as u64);
                    if let Some(d) = self.cur.as_ref().and_then(|c| c.decoder.info.duration) {
                        pos = pos.min(d.saturating_sub(Duration::from_millis(500)));
                    }
                    self.seek(pos);
                }
            }
            Command::PrefetchNext(path) => self.prefetch(path),
            Command::SetOutput { ring, rate } => self.set_output(ring, rate),
            Command::Shutdown => return false,
        }
        true
    }

    fn position(&self) -> Duration {
        let rate = self.out_rate.max(1) as f64;
        Duration::from_secs_f64(self.shared.position_frames() as f64 / rate)
    }

    fn open_current(&mut self, decoder: Decoder) -> anyhow::Result<()> {
        let resampler = Resampler::new(decoder.info.sample_rate, self.out_rate)?;
        self.cur = Some(Current { decoder, resampler });
        self.eof = false;
        Ok(())
    }

    fn load(&mut self, path: PathBuf, play: bool) {
        self.flush();
        self.cur = None;
        self.cancel_prefetch();
        self.eof = false;
        let opened = Decoder::open(&path).and_then(|d| {
            let info = d.info.clone();
            self.open_current(d)?;
            Ok(info)
        });
        match opened {
            Ok(info) => {
                self.shared
                    .track_start
                    .store(self.consumed(), Ordering::Release);
                self.last_path = Some(info.path.clone());
                self.emit(Event::Loaded(info));
                self.set_state(if play {
                    PlayerState::Playing
                } else {
                    PlayerState::Stopped
                });
            }
            Err(e) => {
                tracing::warn!("cannot play {}: {e:#}", path.display());
                self.last_path = Some(path.clone());
                self.set_state(PlayerState::Stopped);
                self.emit(Event::LoadFailed {
                    path,
                    msg: format!("{e:#}"),
                    play,
                });
            }
        }
    }

    fn stop(&mut self) {
        self.flush();
        self.cur = None;
        self.cancel_prefetch();
        self.eof = false;
        self.shared
            .track_start
            .store(self.consumed(), Ordering::Release);
        self.set_state(PlayerState::Stopped);
    }

    fn seek(&mut self, pos: Duration) {
        if self.cur.is_none() {
            return;
        }
        // The current decoder may already be a gapless successor whose first
        // frames are about to be discarded: it becomes current right now.
        while let Some((_, info)) = self.transitions.pop_front() {
            self.advanced(info);
        }
        self.flush();
        let cur = self.cur.as_mut().expect("current");
        match cur.decoder.seek(pos) {
            Ok(reached) => {
                cur.resampler.reset();
                self.eof = false;
                let frames = (reached.as_secs_f64() * self.out_rate as f64) as i64;
                self.shared
                    .track_start
                    .store(self.consumed() - frames, Ordering::Release);
                self.emit(Event::Seeked(reached));
            }
            Err(e) => {
                tracing::warn!("seek: {e:#}");
                self.emit(Event::Error {
                    path: None,
                    msg: format!("{e:#}"),
                });
            }
        }
    }

    /// Switches to a reopened output. Whatever sat in the old ring is gone, so
    /// the current track restarts from the position last heard, resampled to
    /// the new rate. The state is kept: paused stays paused. The prefetched
    /// decoder is dropped; the app announces the next track again.
    fn set_output(&mut self, ring: rtrb::Producer<f32>, rate: u32) {
        // A pending gapless successor is already the current decoder and none
        // of it was heard yet: it starts from zero (`seek` announces it).
        let pos = if self.transitions.is_empty() {
            self.position()
        } else {
            Duration::ZERO
        };
        self.ring = ring;
        self.out_rate = rate;
        self.shared.out_rate.store(rate, Ordering::Relaxed);
        self.cancel_prefetch();
        // Frames resampled for the old rate.
        self.buf.clear();
        self.buf_pos = 0;
        self.pushed_total = self.consumed();
        let Some(cur) = self.cur.as_mut() else {
            self.transitions.clear();
            return;
        };
        if let Err(e) = rebuild_resampler(cur, rate) {
            tracing::warn!("resampler for {rate} Hz: {e:#}");
            self.stop();
            self.emit(Event::Error {
                path: None,
                msg: format!("{e:#}"),
            });
            return;
        }
        self.seek(pos);
    }

    /// Drops the prefetched decoder and makes any open in flight stale.
    fn cancel_prefetch(&mut self) {
        self.prefetch_gen += 1;
        self.next = None;
        self.pending = None;
    }

    /// Opens `path` on a short-lived thread: a slow disk must not stall
    /// `fill`, and the ring holds only 0.3 s. The result arrives through
    /// `prefetch_rx` and lands in `prefetched`.
    fn prefetch(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else {
            self.cancel_prefetch();
            return;
        };
        if self.next.as_ref().is_some_and(|(p, _)| *p == path)
            || self.pending.as_ref() == Some(&path)
        {
            return;
        }
        self.cancel_prefetch();
        let generation = self.prefetch_gen;
        let tx = self.prefetch_tx.clone();
        let p = path.clone();
        let spawned = std::thread::Builder::new()
            .name("prefetch".into())
            .spawn(move || {
                let r = Decoder::open(&p);
                let _ = tx.send((generation, p, r));
            });
        match spawned {
            Ok(_) => self.pending = Some(path),
            Err(e) => tracing::warn!("spawn prefetch: {e}"),
        }
    }

    fn prefetched(&mut self, (generation, path, result): Prefetched) {
        if generation != self.prefetch_gen {
            return;
        }
        self.pending = None;
        match result {
            Ok(d) => self.next = Some((path, d)),
            Err(e) => tracing::debug!("prefetch {}: {e:#}", path.display()),
        }
    }

    /// Pushes buffered frames; returns true if everything buffered was pushed.
    fn push_buf(&mut self) -> bool {
        let pending = &self.buf[self.buf_pos..];
        if pending.is_empty() {
            return true;
        }
        let room = self.ring.slots() & !1;
        let n = room.min(pending.len());
        if n == 0 {
            return false;
        }
        let (pushed, _) = self.ring.push_partial_slice(&pending[..n]);
        let pushed = pushed.len();
        self.buf_pos += pushed;
        self.pushed_total += (pushed / 2) as i64;
        if self.buf_pos >= self.buf.len() {
            self.buf.clear();
            self.buf_pos = 0;
            true
        } else {
            false
        }
    }

    fn fill(&mut self) {
        loop {
            if !self.push_buf() {
                return;
            }
            if self.eof {
                if self.next.is_some() {
                    self.switch_to_next();
                    continue;
                }
                return;
            }
            let Some(cur) = self.cur.as_mut() else {
                return;
            };
            self.scratch.clear();
            match cur.decoder.next_frames(&mut self.scratch) {
                Ok(true) => {
                    // Chained streams may change rate mid-track: flush the
                    // old rate's tail, then resample the rest at the new rate.
                    if cur.decoder.info.sample_rate != cur.resampler.in_rate() {
                        let rebuilt = cur
                            .resampler
                            .finish(&mut self.buf)
                            .and_then(|()| rebuild_resampler(cur, self.out_rate));
                        if let Err(e) = rebuilt {
                            tracing::warn!("resampler rebuild: {e:#}");
                            self.eof = true;
                            continue;
                        }
                    }
                    if let Err(e) = cur.resampler.process(&self.scratch, &mut self.buf) {
                        tracing::warn!("resampler: {e:#}");
                        self.eof = true;
                    }
                }
                Ok(false) => {
                    if let Err(e) = cur.resampler.finish(&mut self.buf) {
                        tracing::warn!("resampler finish: {e:#}");
                    }
                    self.eof = true;
                }
                Err(e) => {
                    let path = cur.decoder.info.path.clone();
                    tracing::warn!("decode {}: {e:#}", path.display());
                    self.emit(Event::Error {
                        path: Some(path),
                        msg: format!("{e:#}"),
                    });
                    self.eof = true;
                }
            }
        }
    }

    fn switch_to_next(&mut self) {
        let Some((_, decoder)) = self.next.take() else {
            return;
        };
        let info = decoder.info.clone();
        // All frames of the previous track (pushed or still buffered) precede this mark.
        let mark = self.pushed_total + ((self.buf.len() - self.buf_pos) / 2) as i64;
        match self.open_current(decoder) {
            Ok(()) => self.transitions.push_back((mark, info)),
            Err(e) => {
                tracing::warn!("gapless switch failed: {e:#}");
                self.eof = true;
            }
        }
    }

    /// Logs the xrun count at most every `XRUN_LOG_INTERVAL`, when it moved.
    fn log_xruns(&mut self) {
        if self.last_xrun_log.elapsed() < XRUN_LOG_INTERVAL {
            return;
        }
        let n = self.shared.xruns.load(Ordering::Relaxed);
        if n != self.last_xruns {
            tracing::debug!("{} xruns ({n} total)", n - self.last_xruns);
            self.last_xruns = n;
        }
        self.last_xrun_log = Instant::now();
    }

    fn check_progress(&mut self) {
        self.log_xruns();
        let consumed = self.consumed();
        while let Some(&(mark, _)) = self.transitions.front() {
            if consumed < mark {
                break;
            }
            let (mark, info) = self.transitions.pop_front().expect("front");
            self.shared.track_start.store(mark, Ordering::Release);
            self.advanced(info);
        }
        if self.state == PlayerState::Playing
            && self.eof
            && self.next.is_none()
            && self.buf_pos >= self.buf.len()
            && consumed >= self.pushed_total
        {
            self.cur = None;
            self.eof = false;
            self.set_state(PlayerState::Stopped);
            self.emit(Event::TrackEnded);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    /// Simulates the realtime callback at roughly 4x real time.
    fn fake_callback(shared: Arc<Shared>, mut ring: rtrb::Consumer<f32>, stop: Arc<AtomicBool>) {
        let mut buf = vec![0.0f32; 2048];
        while !stop.load(Ordering::Relaxed) {
            if shared.flush.load(Ordering::Acquire) {
                let n = ring.slots();
                if let Ok(c) = ring.read_chunk(n) {
                    c.commit_all();
                }
                shared.flush.store(false, Ordering::Release);
            }
            if !shared.paused.load(Ordering::Relaxed) {
                let (got, _) = ring.pop_partial_slice(&mut buf);
                let frames = got.len() / 2;
                shared.consumed.fetch_add(frames as u64, Ordering::AcqRel);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait_for(rx: &Receiver<Event>, pred: impl Fn(&Event) -> bool, secs: u64) -> Event {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(ev) if pred(&ev) => return ev,
                Ok(_) => continue,
                Err(_) => panic!("timed out waiting for event"),
            }
        }
    }

    #[test]
    fn resume_loads_paused_at_position() {
        let dir = std::env::temp_dir().join(format!("khz-resume-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        crate::library::scanner::tests::write_wav(&path, 8000 * 4);

        let shared = Arc::new(Shared::new(1.0, 0.0));
        let (ring_tx, ring_rx) = rtrb::RingBuffer::new(48000 * 2 * 3 / 10);
        let stop = Arc::new(AtomicBool::new(false));
        let cb = {
            let (s, st) = (shared.clone(), stop.clone());
            std::thread::spawn(move || fake_callback(s, ring_rx, st))
        };
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let eng = {
            let s = shared.clone();
            std::thread::spawn(move || run(rx, ev_tx, s, ring_tx, egui::Context::default()))
        };
        let pos = || Duration::from_secs_f64(shared.position_frames() as f64 / 48000.0);

        tx.send(Command::Resume { path, at: Duration::from_millis(2500) }).unwrap();
        wait_for(&ev_rx, |e| matches!(e, Event::StateChanged(PlayerState::Paused)), 5);
        std::thread::sleep(Duration::from_millis(100));
        assert!((pos().as_secs_f64() - 2.5).abs() < 0.05, "{:?}", pos());

        tx.send(Command::Play).unwrap();
        wait_for(&ev_rx, |e| matches!(e, Event::StateChanged(PlayerState::Playing)), 5);
        std::thread::sleep(Duration::from_millis(100));
        assert!(pos() > Duration::from_millis(2500), "{:?}", pos());

        tx.send(Command::Shutdown).unwrap();
        eng.join().unwrap();
        stop.store(true, Ordering::Relaxed);
        cb.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn idle_engine_wakes_on_commands_and_exits_on_disconnect() {
        let dir = std::env::temp_dir().join(format!("khz-idle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        crate::library::scanner::tests::write_wav(&path, 8000);

        let shared = Arc::new(Shared::new(1.0, 0.0));
        let (ring_tx, ring_rx) = rtrb::RingBuffer::new(48000 * 2 * 3 / 10);
        let stop = Arc::new(AtomicBool::new(false));
        let cb = {
            let (s, st) = (shared.clone(), stop.clone());
            std::thread::spawn(move || fake_callback(s, ring_rx, st))
        };
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let eng = std::thread::spawn(move || run(rx, ev_tx, shared, ring_tx, egui::Context::default()));

        // Stopped: the engine is blocked on the channel and must still react.
        std::thread::sleep(Duration::from_millis(50));
        tx.send(Command::Load { path, play: true }).unwrap();
        wait_for(&ev_rx, |e| matches!(e, Event::StateChanged(PlayerState::Playing)), 5);
        tx.send(Command::Stop).unwrap();
        wait_for(&ev_rx, |e| matches!(e, Event::StateChanged(PlayerState::Stopped)), 5);

        // Dropping the sender while blocked ends the thread.
        let t0 = Instant::now();
        drop(tx);
        eng.join().unwrap();
        assert!(t0.elapsed() < Duration::from_secs(1));
        stop.store(true, Ordering::Relaxed);
        cb.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn test_engine() -> (Engine, Receiver<Event>, rtrb::Consumer<f32>) {
        let shared = Arc::new(Shared::new(1.0, 0.0));
        let (ring, ring_rx) = rtrb::RingBuffer::new(48000 * 2 * 3 / 10);
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (prefetch_tx, prefetch_rx) = crossbeam_channel::unbounded();
        let e = Engine {
            events: ev_tx,
            out_rate: shared.out_rate(),
            shared,
            ring,
            repaint: egui::Context::default(),
            state: PlayerState::Stopped,
            cur: None,
            last_path: None,
            next: None,
            prefetch_tx,
            prefetch_rx,
            prefetch_gen: 0,
            pending: None,
            buf: Vec::new(),
            buf_pos: 0,
            scratch: Vec::new(),
            pushed_total: 0,
            transitions: VecDeque::new(),
            eof: false,
            last_xruns: 0,
            last_xrun_log: Instant::now(),
        };
        (e, ev_rx, ring_rx)
    }

    /// Waits for the next background open to report (any generation).
    fn recv_prefetch(e: &Engine) -> Prefetched {
        e.prefetch_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("prefetch result")
    }

    /// `prefetch` and wait until its result is applied, as the run loop would.
    fn prefetch_now(e: &mut Engine, path: PathBuf) {
        e.prefetch(Some(path));
        let r = recv_prefetch(e);
        e.prefetched(r);
        assert!(e.pending.is_none() && e.next.is_some());
    }

    #[test]
    fn prefetch_result_from_old_generation_is_ignored() {
        let dir = crate::config::test_dir("engine-prefetch-gen");
        let [a, b] = ["a.wav", "b.wav"].map(|n| dir.join(n));
        for p in [&a, &b] {
            crate::library::scanner::tests::write_wav(p, 800);
        }
        let (mut e, _events, _ring) = test_engine();
        e.prefetch(Some(a.clone()));
        e.prefetch(Some(b.clone()));
        // Same path again while in flight: no new open.
        e.prefetch(Some(b.clone()));
        assert_eq!(e.pending.as_ref(), Some(&b));
        // Deliver A's stale result last, the worst order.
        let mut results = [recv_prefetch(&e), recv_prefetch(&e)];
        results.sort_by_key(|r| r.1 == a);
        for r in results {
            e.prefetched(r);
        }
        assert!(e.prefetch_rx.try_recv().is_err());
        assert!(e.pending.is_none());
        assert_eq!(e.next.as_ref().map(|(p, _)| p), Some(&b));

        // A stop makes an open in flight stale too.
        e.prefetch(Some(a));
        e.stop();
        let r = recv_prefetch(&e);
        e.prefetched(r);
        assert!(e.next.is_none() && e.pending.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_during_pending_transition_emits_no_advanced() {
        let dir = crate::config::test_dir("engine-pending");
        let [a, b, c] = ["a.wav", "b.wav", "c.wav"].map(|n| dir.join(n));
        for p in [&a, &b, &c] {
            crate::library::scanner::tests::write_wav(p, 800);
        }
        let (mut e, events, mut ring) = test_engine();
        e.load(a, true);
        prefetch_now(&mut e, b.clone());
        // A is shorter than the ring: decoding runs into B without any of A
        // being played, so the switch to B is pending.
        e.fill();
        assert_eq!(e.transitions.len(), 1);
        assert_eq!(e.last_path.as_deref(), Some(dir.join("a.wav").as_path()));

        e.load(c.clone(), true);
        let evs: Vec<Event> = events.try_iter().collect();
        assert!(!evs.iter().any(|ev| matches!(ev, Event::Advanced(_))), "{evs:?}");
        assert!(matches!(evs.last(), Some(Event::Loaded(i)) if i.path == c), "{evs:?}");

        // Stop drops a pending switch too, and Play then restarts the audible track.
        while ring.pop().is_ok() {}
        prefetch_now(&mut e, b);
        e.fill();
        assert_eq!(e.transitions.len(), 1);
        e.stop();
        assert!(!events.try_iter().any(|ev| matches!(ev, Event::Advanced(_))));
        assert_eq!(e.last_path.as_ref(), Some(&c));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn set_output_switches_ring_and_rate() {
        let dir = crate::config::test_dir("engine-set-output");
        let path = dir.join("t.wav");
        crate::library::scanner::tests::write_wav(&path, 8000 * 2);
        let (mut e, events, mut old_rx) = test_engine();
        e.load(path, true);
        e.fill();
        // The callback played 100 ms of it.
        let heard = 4800;
        for _ in 0..heard * 2 {
            old_rx.pop().unwrap();
        }
        e.shared.consumed.fetch_add(heard, Ordering::AcqRel);
        assert!((e.position().as_secs_f64() - 0.1).abs() < 1e-6);
        let old_left = old_rx.slots();
        events.try_iter().for_each(drop);

        let (ring, new_rx) = rtrb::RingBuffer::new(44100 * 2 * 3 / 10);
        assert!(e.handle(Command::SetOutput { ring, rate: 44100 }));
        assert_eq!(e.out_rate, 44100);
        assert_eq!(e.shared.out_rate(), 44100);
        assert_eq!(e.state, PlayerState::Playing);
        let pos = e.position().as_secs_f64();
        assert!((pos - 0.1).abs() < 0.010, "position {pos}");
        assert!(events.try_iter().any(|ev| matches!(ev, Event::Seeked(_))));

        e.fill();
        assert!(old_rx.is_abandoned());
        assert_eq!(old_rx.slots(), old_left, "frames went to the old ring");
        let queued = new_rx.slots();
        assert!(queued > 0, "nothing reached the new ring");
        assert_eq!(e.pushed_total - e.consumed(), queued as i64 / 2);
        // 1.9 s left at 44.1 kHz is far more than the ring holds.
        assert_eq!(queued, 44100 * 2 * 3 / 10);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn set_output_during_pending_transition_restarts_successor() {
        let dir = crate::config::test_dir("engine-set-output-pending");
        let [a, b] = ["a.wav", "b.wav"].map(|n| dir.join(n));
        for p in [&a, &b] {
            crate::library::scanner::tests::write_wav(p, 800);
        }
        let (mut e, events, _old_rx) = test_engine();
        e.load(a, true);
        prefetch_now(&mut e, b.clone());
        e.fill();
        assert_eq!(e.transitions.len(), 1);
        events.try_iter().for_each(drop);

        let (ring, _new_rx) = rtrb::RingBuffer::new(1024);
        e.handle(Command::SetOutput { ring, rate: 44100 });
        let evs: Vec<Event> = events.try_iter().collect();
        assert!(matches!(evs.first(), Some(Event::Advanced(i)) if i.path == b), "{evs:?}");
        assert!(e.transitions.is_empty());
        assert_eq!(e.last_path.as_ref(), Some(&b));
        assert_eq!(e.position(), Duration::ZERO);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn flush_returns_fast_when_device_lost() {
        let (mut e, _events, _ring) = test_engine();
        e.shared.device_lost.store(true, Ordering::Release);
        // No callback runs, so the flag is never acknowledged.
        let t0 = Instant::now();
        e.flush();
        assert!(t0.elapsed() < Duration::from_millis(5), "{:?}", t0.elapsed());
        assert!(e.shared.flush.load(Ordering::Acquire));
    }

    #[test]
    fn load_failure_emits_load_failed() {
        let (mut e, events, _ring) = test_engine();
        let path = PathBuf::from("/nonexistent/khz-test.mp3");
        e.load(path.clone(), true);
        let evs: Vec<Event> = events.try_iter().collect();
        assert!(
            evs.iter().any(|ev| matches!(ev, Event::LoadFailed { path: p, play: true, .. } if *p == path)),
            "{evs:?}"
        );
        assert_eq!(e.state, PlayerState::Stopped);
    }

    /// Plays like the realtime callback: acknowledges a flush, then takes up
    /// to `frames` frames from the ring and advances `consumed` by as many.
    fn consume(e: &Engine, ring: &mut rtrb::Consumer<f32>, frames: usize) -> usize {
        if e.shared.flush.load(Ordering::Acquire) {
            let n = ring.slots();
            ring.read_chunk(n).unwrap().commit_all();
            e.shared.flush.store(false, Ordering::Release);
        }
        let n = ring.slots().min(frames * 2) & !1;
        ring.read_chunk(n).unwrap().commit_all();
        e.shared.consumed.fetch_add(n as u64 / 2, Ordering::AcqRel);
        n / 2
    }

    #[test]
    fn gapless_across_rates_pushes_every_frame_and_advances_on_the_mark() {
        let dir = crate::config::test_dir("engine-gapless-rates");
        let [a, b] = ["a.wav", "b.wav"].map(|n| dir.join(n));
        // 8000 frames each at different rates: 1 s and 0.5 s.
        crate::library::scanner::tests::write_wav_rate(&a, 8000, 8000);
        crate::library::scanner::tests::write_wav_rate(&b, 8000, 16000);
        let (mut e, events, mut ring) = test_engine();
        assert_eq!(e.out_rate, 48000);
        e.load(a, true);
        // No callback ran during the load: ack its flush now, on an empty ring.
        consume(&e, &mut ring, 0);
        prefetch_now(&mut e, b.clone());
        events.try_iter().for_each(drop);

        let mut mark = None;
        let mut advanced_at = None;
        let mut ended = 0;
        // A step that does not divide the mark, so it falls inside one.
        for _ in 0..1000 {
            e.fill();
            if mark.is_none() {
                mark = e.transitions.front().map(|&(m, _)| m);
            }
            let consumed = e.consumed();
            e.check_progress();
            for ev in events.try_iter() {
                match ev {
                    Event::Advanced(info) => {
                        assert_eq!(info.path, b);
                        assert!(advanced_at.is_none(), "Advanced twice");
                        advanced_at = Some(consumed);
                    }
                    Event::TrackEnded => ended += 1,
                    _ => {}
                }
            }
            if let Some(m) = mark {
                // Fires on the first check at or past the mark, never before.
                assert_eq!(
                    advanced_at.is_some(),
                    consumed >= m,
                    "consumed {consumed}, mark {m}"
                );
            }
            if ended > 0 {
                break;
            }
            consume(&e, &mut ring, 777);
        }
        let mark = mark.expect("no gapless transition");
        let advanced_at = advanced_at.expect("no Advanced");
        assert!(advanced_at >= mark && advanced_at < mark + 777);
        assert_eq!(ended, 1);
        // Everything decoded was pushed and played, A and B resampled to 48 kHz.
        assert!((mark - 48000).abs() <= 2, "A pushed {mark} frames");
        assert!(
            (e.pushed_total - mark - 24000).abs() <= 2,
            "B pushed {} frames",
            e.pushed_total - mark
        );
        assert!(
            (e.pushed_total - 72000).abs() <= 4,
            "pushed {}",
            e.pushed_total
        );
        assert_eq!(e.consumed(), e.pushed_total);
        assert_eq!(ring.slots(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn track_ends_once_after_the_ring_drains() {
        let dir = crate::config::test_dir("engine-track-ended");
        let path = dir.join("t.wav");
        crate::library::scanner::tests::write_wav(&path, 8000);
        let (mut e, events, mut ring) = test_engine();
        e.load(path, true);
        // No callback ran during the load: ack its flush now, on an empty ring.
        consume(&e, &mut ring, 0);
        events.try_iter().for_each(drop);

        // Decode to the end while the ring is drained in steps.
        loop {
            e.fill();
            e.check_progress();
            if e.eof && e.buf.is_empty() {
                break;
            }
            consume(&e, &mut ring, 4096);
        }
        assert!(
            ring.slots() > 2,
            "ring drained already, the test proves nothing"
        );
        // Decoding is done but the ring still plays: not ended yet. Leave one frame.
        while ring.slots() > 2 {
            e.check_progress();
            assert!(!events.try_iter().any(|ev| matches!(ev, Event::TrackEnded)));
            let step = (ring.slots() / 2 - 1).min(1000);
            consume(&e, &mut ring, step);
        }
        // One frame left: still playing.
        e.check_progress();
        assert!(!events.try_iter().any(|ev| matches!(ev, Event::TrackEnded)));
        assert_eq!(consume(&e, &mut ring, 1000), 1);
        for _ in 0..3 {
            e.fill();
            e.check_progress();
        }
        let ended = events
            .try_iter()
            .filter(|ev| matches!(ev, Event::TrackEnded))
            .count();
        assert_eq!(ended, 1);
        assert_eq!(e.state, PlayerState::Stopped);
        assert!(e.cur.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Documents today's behaviour: a relative seek past the end stops half a
    /// second short of it. A24 turns this into Next; update the test then.
    #[test]
    fn seek_rel_past_end_clamps_before_the_end() {
        let dir = crate::config::test_dir("engine-seek-rel");
        let path = dir.join("t.wav");
        crate::library::scanner::tests::write_wav(&path, 8000 * 4);
        let (mut e, events, _ring) = test_engine();
        e.load(path, true);
        events.try_iter().for_each(drop);

        e.handle(Command::SeekRel(60_000));
        let seeked: Vec<Duration> = events
            .try_iter()
            .filter_map(|ev| match ev {
                Event::Seeked(d) => Some(d),
                _ => None,
            })
            .collect();
        assert_eq!(seeked, [Duration::from_millis(3500)]);
        assert_eq!(e.position(), Duration::from_millis(3500));
        assert_eq!(e.state, PlayerState::Playing);

        // And before the start, to zero.
        e.handle(Command::SeekRel(-60_000));
        assert!(
            events
                .try_iter()
                .any(|ev| matches!(ev, Event::Seeked(d) if d.is_zero()))
        );
        assert_eq!(e.position(), Duration::ZERO);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Needs a chained Ogg of two or more streams, ideally at different rates
    /// (`cat a.ogg b.ogg > chained.ogg`):
    /// `RMP_TEST_CHAINED_OGG=/path/chained.ogg cargo test chained_ogg -- --ignored`.
    #[test]
    #[ignore]
    fn chained_ogg_real_file() {
        let path =
            PathBuf::from(std::env::var("RMP_TEST_CHAINED_OGG").expect("RMP_TEST_CHAINED_OGG"));
        // The decoder runs through every stream; record (rate, frames) runs.
        let mut d = Decoder::open(&path).unwrap();
        let first = d.info.duration.expect("duration").as_secs_f64();
        let mut runs: Vec<(u32, u64)> = Vec::new();
        let mut frames = Vec::new();
        loop {
            frames.clear();
            if !d.next_frames(&mut frames).unwrap() {
                break;
            }
            let (rate, n) = (d.info.sample_rate, frames.len() as u64 / 2);
            match runs.last_mut() {
                Some((r, f)) if *r == rate => *f += n,
                _ => runs.push((rate, n)),
            }
        }
        let secs: f64 = runs.iter().map(|&(r, f)| f as f64 / r as f64).sum();
        println!("first stream {first:.3} s, decoded {secs:.3} s, runs {runs:?}");
        assert!(secs > first + 0.5, "stopped after the first stream");

        // The engine resamples each run to the output rate without losing frames.
        let (mut e, _events, mut ring) = test_engine();
        e.load(path, true);
        loop {
            e.fill();
            while ring.pop().is_ok() {}
            if e.eof && e.buf.is_empty() {
                break;
            }
        }
        let out = e.out_rate as f64;
        let expected: f64 = runs
            .iter()
            .map(|&(r, f)| (f as f64 * out / r as f64).round())
            .sum();
        let slack = 2 * runs.len() as i64;
        assert!(
            (e.pushed_total - expected as i64).abs() <= slack,
            "pushed {} frames, expected {expected}",
            e.pushed_total
        );
    }

    /// Needs a real file: `RMP_TEST_FILE=/path/song.mp3 cargo test engine_real -- --ignored`.
    #[test]
    #[ignore]
    fn engine_real_file() {
        let path = PathBuf::from(std::env::var("RMP_TEST_FILE").expect("RMP_TEST_FILE"));
        let shared = Arc::new(Shared::new(1.0, 0.0));
        let (ring_tx, ring_rx) = rtrb::RingBuffer::new(48000 * 2 * 3 / 10);
        let stop = Arc::new(AtomicBool::new(false));
        let cb = {
            let (s, st) = (shared.clone(), stop.clone());
            std::thread::spawn(move || fake_callback(s, ring_rx, st))
        };
        let (tx, rx) = crossbeam_channel::unbounded();
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let eng = {
            let s = shared.clone();
            std::thread::spawn(move || run(rx, ev_tx, s, ring_tx, egui::Context::default()))
        };
        let pos = || Duration::from_secs_f64(shared.position_frames() as f64 / 48000.0);

        tx.send(Command::Load { path: path.clone(), play: true }).unwrap();
        let Event::Loaded(info) = wait_for(&ev_rx, |e| matches!(e, Event::Loaded(_)), 5) else {
            unreachable!()
        };
        let dur = info.duration.expect("duration");
        println!("loaded {info:?}");
        std::thread::sleep(Duration::from_millis(500));
        assert!(pos() > Duration::from_millis(200), "position does not advance: {:?}", pos());

        // Accurate seek.
        let target = dur / 2;
        tx.send(Command::Seek(target)).unwrap();
        let Event::Seeked(reached) = wait_for(&ev_rx, |e| matches!(e, Event::Seeked(_)), 5) else {
            unreachable!()
        };
        let diff = reached.as_secs_f64() - target.as_secs_f64();
        assert!(diff.abs() < 0.1, "seek reached {reached:?} for {target:?}");
        assert!((pos().as_secs_f64() - target.as_secs_f64()).abs() < 0.2);

        // Pause holds the position.
        tx.send(Command::Pause).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let p1 = pos();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(p1, pos());
        tx.send(Command::Play).unwrap();

        // Gapless: prefetch the same file and run into the end.
        tx.send(Command::PrefetchNext(Some(path.clone()))).unwrap();
        tx.send(Command::Seek(dur.saturating_sub(Duration::from_secs(2)))).unwrap();
        wait_for(&ev_rx, |e| matches!(e, Event::Advanced(_)), 10);
        assert!(pos() < Duration::from_secs(2), "position not reset: {:?}", pos());

        // Without prefetch the track ends and the engine stops.
        tx.send(Command::Seek(dur.saturating_sub(Duration::from_secs(1)))).unwrap();
        wait_for(&ev_rx, |e| matches!(e, Event::TrackEnded), 10);

        tx.send(Command::Shutdown).unwrap();
        eng.join().unwrap();
        stop.store(true, Ordering::Relaxed);
        cb.join().unwrap();
    }
}
