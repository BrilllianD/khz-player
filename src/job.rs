//! Shared contract for background jobs: a named thread, an event channel, cooperative cancel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender, TryRecvError};

/// An event that ends its job; nothing after it is delivered.
pub trait Terminal {
    fn is_terminal(&self) -> bool;
}

/// Cancel flag shared between the app and the job body.
#[derive(Clone)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Sending half handed to the job body. A body that returns without a terminal event is `Lost`.
pub struct Sink<E> {
    tx: Sender<E>,
    repaint: egui::Context,
}

impl<E> Sink<E> {
    pub fn send(&self, ev: E) {
        let _ = self.tx.send(ev);
        self.repaint.request_repaint();
    }
}

#[derive(Debug)]
pub enum Poll<E> {
    Event(E),
    /// The thread ended or never started without a terminal event.
    Lost,
}

pub struct Job<E> {
    rx: Receiver<E>,
    cancel: Cancel,
    finished: bool,
}

pub fn spawn<E, F>(name: &str, repaint: egui::Context, body: F) -> Job<E>
where
    E: Terminal + Send + 'static,
    F: FnOnce(&Sink<E>, &Cancel) + Send + 'static,
{
    let (tx, rx) = crossbeam_channel::unbounded();
    let cancel = Cancel(Arc::new(AtomicBool::new(false)));
    let c = cancel.clone();
    let sink = Sink { tx, repaint };
    let spawned = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || body(&sink, &c));
    if let Err(e) = spawned {
        tracing::error!("spawn {name}: {e}");
    }
    Job {
        rx,
        cancel,
        finished: false,
    }
}

impl<E: Terminal> Job<E> {
    /// Drains pending events without blocking. Stops after a terminal event or `Lost`.
    pub fn poll(&mut self) -> Vec<Poll<E>> {
        let mut out = Vec::new();
        if self.finished {
            return out;
        }
        loop {
            match self.rx.try_recv() {
                Ok(ev) => {
                    let end = ev.is_terminal();
                    out.push(Poll::Event(ev));
                    if end {
                        self.finished = true;
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    out.push(Poll::Lost);
                    self.finished = true;
                    break;
                }
            }
        }
        out
    }

    pub fn cancel(&self) {
        self.cancel.0.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[derive(Debug, PartialEq)]
    enum Ev {
        Step(u32),
        End,
    }

    impl Terminal for Ev {
        fn is_terminal(&self) -> bool {
            matches!(self, Ev::End)
        }
    }

    fn drain(job: &mut Job<Ev>) -> Vec<Poll<Ev>> {
        let start = Instant::now();
        let mut out = Vec::new();
        while !job.is_finished() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "job did not finish"
            );
            out.extend(job.poll());
            std::thread::sleep(Duration::from_millis(1));
        }
        out
    }

    #[test]
    fn terminal_event_ends_job() {
        let mut job = spawn("t", egui::Context::default(), |sink, _| {
            sink.send(Ev::Step(1));
            sink.send(Ev::End);
            sink.send(Ev::Step(2));
        });
        let got = drain(&mut job);
        assert_eq!(got.len(), 2);
        assert!(matches!(got[0], Poll::Event(Ev::Step(1))));
        assert!(matches!(got[1], Poll::Event(Ev::End)));
        assert!(job.is_finished());
        assert!(job.poll().is_empty());
    }

    #[test]
    fn body_without_terminal_is_lost() {
        let mut job = spawn("t", egui::Context::default(), |sink, _| {
            sink.send(Ev::Step(1));
        });
        let got = drain(&mut job);
        assert_eq!(got.len(), 2);
        assert!(matches!(got[0], Poll::Event(Ev::Step(1))));
        assert!(matches!(got[1], Poll::Lost));
    }

    #[test]
    fn panicking_body_is_lost() {
        let mut job: Job<Ev> = spawn("t", egui::Context::default(), |_, _| {
            panic!("job body panic (expected in test)");
        });
        let got = drain(&mut job);
        assert_eq!(got.len(), 1);
        assert!(matches!(got[0], Poll::Lost));
    }

    #[test]
    fn cancel_reaches_body() {
        let mut job = spawn("t", egui::Context::default(), |sink, cancel| {
            while !cancel.is_set() {
                std::thread::sleep(Duration::from_millis(1));
            }
            sink.send(Ev::End);
        });
        job.cancel();
        let got = drain(&mut job);
        assert_eq!(got.len(), 1);
        assert!(matches!(got[0], Poll::Event(Ev::End)));
    }
}
