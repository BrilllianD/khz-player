//! Watches the Omarchy theme directory and reports changes (debounced).

use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError};
use notify::{RecursiveMode, Watcher};

use crate::theme::Theme;

const DEBOUNCE: Duration = Duration::from_millis(250);

pub struct ThemeWatcher {
    _watcher: notify::RecommendedWatcher,
    pub changed: Receiver<()>,
}

/// Starts watching; returns `None` if the Omarchy state dir does not exist.
pub fn start(repaint: egui::Context) -> Option<ThemeWatcher> {
    let dir = Theme::omarchy_dir()?;
    if !dir.is_dir() {
        return None;
    }
    let (raw_tx, raw_rx) = crossbeam_channel::unbounded::<()>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res
            && !ev.kind.is_access()
        {
            let _ = raw_tx.send(());
        }
    })
    .map_err(|e| tracing::warn!("theme watcher: {e}"))
    .ok()?;
    // Recursive: the theme dir may be swapped as a whole or rewritten in place.
    if let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive) {
        tracing::warn!("watch {}: {e}", dir.display());
        return None;
    }
    let (tx, rx) = crossbeam_channel::bounded::<()>(1);
    std::thread::Builder::new()
        .name("theme-watch".into())
        .spawn(move || {
            while raw_rx.recv().is_ok() {
                // Collapse a burst of events into one reload.
                loop {
                    match raw_rx.recv_timeout(DEBOUNCE) {
                        Ok(()) => continue,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                let _ = tx.try_send(());
                repaint.request_repaint();
            }
        })
        .ok()?;
    tracing::debug!("watching {}", dir.display());
    Some(ThemeWatcher {
        _watcher: watcher,
        changed: rx,
    })
}
