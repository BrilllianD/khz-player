use super::*;

const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);
/// Save anyway after this long of continuous changes (a long slider drag).
const SAVE_CAP: Duration = Duration::from_secs(10);

impl App {
    pub(super) fn save_now(&mut self) {
        let mut error = None;
        let mut saved = false;
        if let Some(db) = &mut self.db {
            for (i, p) in self.playlists.iter_mut().enumerate() {
                if !p.dirty {
                    continue;
                }
                // Cleared even on failure: retrying every second would not help.
                p.dirty = false;
                match db::save_playlist(db, p, i) {
                    Ok(()) => saved = true,
                    Err(e) => error = Some(format!("Could not save playlist {}: {e}", p.name)),
                }
            }
        }
        match error {
            Some(msg) if !self.save_failed => {
                self.save_failed = true;
                self.toast(msg);
            }
            Some(msg) => tracing::warn!("{msg}"),
            None if saved => self.save_failed = false,
            None => {}
        }
        // Remember the list that is playing, so the next start resumes it.
        let list = if self.now.is_some() {
            self.playing_list
        } else {
            self.active
        };
        self.cfg.last_playlist = self.playlists.get(list).map(|p| p.name.clone());
        self.cfg.last_track_index = self.playlists.get(list).and_then(|p| p.current);
        self.cfg.last_position_ms = (self.now.is_some() && self.state != PlayerState::Stopped)
            .then(|| self.audio.position().as_millis() as u64);
        if let Err(e) = self.cfg.save() {
            tracing::warn!("save config: {e}");
        }
        self.cfg_changed = None;
        self.cfg_first_change = None;
    }

    pub(super) fn remember_window(&mut self, ctx: &egui::Context) {
        let h = ctx.content_rect().height().round();
        // Tiled or squeezed heights would reopen a floating window with no room
        // for the playlist; window_size() clamps too, this keeps config sane.
        if self.cfg.show_playlist
            && self.cfg.window.h != Some(h)
            && h >= ui::MAIN_H + ui::PLAYLIST_MIN_H
        {
            self.cfg.window.h = Some(h);
            self.mark_cfg();
        }
        let lib_w = if self.cfg.show_library {
            self.cfg
                .window
                .library_w
                .unwrap_or(ui::LIBRARY_W)
                .max(260.0)
        } else {
            0.0
        };
        let main_w = ctx.content_rect().width().round() - lib_w;
        if self.cfg.window.w != Some(main_w) && main_w >= ui::MAIN_W {
            self.cfg.window.w = Some(main_w);
            self.mark_cfg();
        }
    }
}

/// Debounce: save once changes stop for `SAVE_DEBOUNCE`, or after
/// `SAVE_CAP` of continuous changes.
pub(super) fn should_save(first: Option<Instant>, last: Option<Instant>, now: Instant) -> bool {
    match (first, last) {
        (Some(first), Some(last)) => {
            now.duration_since(last) >= SAVE_DEBOUNCE || now.duration_since(first) >= SAVE_CAP
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub(super) fn debounce_waits_and_caps() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        assert!(!should_save(None, None, ms(5000)));
        // Waits for a quiet second after the last change.
        assert!(!should_save(Some(t0), Some(ms(900)), ms(1500)));
        assert!(should_save(Some(t0), Some(ms(900)), ms(1900)));
        // Changes every 500 ms never go quiet, but the cap saves at 10 s.
        assert!(!should_save(Some(t0), Some(ms(9500)), ms(9900)));
        assert!(should_save(Some(t0), Some(ms(9500)), ms(10_000)));
    }
}
