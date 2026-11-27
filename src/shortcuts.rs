//! Global keyboard shortcuts (Winamp layout).

use egui::{Key, Modifiers};

use crate::playlist::Move;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Prev,
    Play,
    /// Winamp's C: pause, or resume when paused.
    PauseToggle,
    Stop,
    Next,
    TogglePlay,
    SeekRel(i64),
    VolumeDelta(f32),
    CycleRepeat,
    ToggleShuffle,
    ToggleEq,
    TogglePlaylist,
    ToggleLibrary,
    AddFiles,
    /// Ctrl+L: download a URL with yt-dlp.
    DownloadUrl,
    Jump,
    RemoveSelected,
    /// Q: queue or unqueue the selection.
    EnqueueSelected,
    /// Ctrl+V: stop when the current track ends (toggle).
    StopAfterCurrent,
    /// Ctrl+S: start or cancel the sleep timer.
    SleepTimer,
    PlaySelected,
    /// Alt+3: file info of the cursor track (or the playing one).
    FileInfo,
    SelectAll,
    Settings,
    /// Elapsed / remaining time on the digits.
    ToggleTimeDisplay,
    /// Ctrl+D: UI at double size (toggle).
    ToggleDoubleSize,
    /// Alt+V: raise the video window.
    ShowVideo,
    /// Move the playlist cursor; the bool extends the selection (Shift).
    Cursor(Move, bool),
    /// Move the selected block one row: -1 up, +1 down.
    MoveSelected(isize),
}

const BINDINGS: &[(Modifiers, Key, Action)] = &[
    (Modifiers::NONE, Key::Z, Action::Prev),
    (Modifiers::NONE, Key::X, Action::Play),
    (Modifiers::NONE, Key::C, Action::PauseToggle),
    (Modifiers::ALT, Key::V, Action::ShowVideo),
    (Modifiers::NONE, Key::V, Action::Stop),
    (Modifiers::NONE, Key::B, Action::Next),
    (Modifiers::NONE, Key::Space, Action::TogglePlay),
    (Modifiers::NONE, Key::ArrowLeft, Action::SeekRel(-5000)),
    (Modifiers::NONE, Key::ArrowRight, Action::SeekRel(5000)),
    (Modifiers::NONE, Key::ArrowUp, Action::VolumeDelta(0.02)),
    (Modifiers::NONE, Key::ArrowDown, Action::VolumeDelta(-0.02)),
    (Modifiers::NONE, Key::Plus, Action::VolumeDelta(0.02)),
    (Modifiers::NONE, Key::Equals, Action::VolumeDelta(0.02)),
    (Modifiers::NONE, Key::Minus, Action::VolumeDelta(-0.02)),
    (Modifiers::NONE, Key::R, Action::CycleRepeat),
    (Modifiers::NONE, Key::S, Action::ToggleShuffle),
    (Modifiers::ALT, Key::G, Action::ToggleEq),
    (Modifiers::ALT, Key::E, Action::TogglePlaylist),
    (Modifiers::ALT, Key::L, Action::ToggleLibrary),
    (Modifiers::ALT, Key::Num3, Action::FileInfo),
    (Modifiers::NONE, Key::L, Action::AddFiles),
    (Modifiers::NONE, Key::J, Action::Jump),
    (Modifiers::NONE, Key::Q, Action::EnqueueSelected),
    (Modifiers::NONE, Key::Delete, Action::RemoveSelected),
    (Modifiers::NONE, Key::Enter, Action::PlaySelected),
    (Modifiers::CTRL, Key::A, Action::SelectAll),
    (Modifiers::CTRL, Key::L, Action::DownloadUrl),
    (Modifiers::CTRL, Key::P, Action::Settings),
    (Modifiers::CTRL, Key::T, Action::ToggleTimeDisplay),
    (Modifiers::CTRL, Key::S, Action::SleepTimer),
    (Modifiers::CTRL, Key::D, Action::ToggleDoubleSize),
];

/// Matched on key release. egui-winit turns a Ctrl+V press into `Event::Paste`
/// (or into nothing when the clipboard is empty) and returns early, so only
/// the release reaches egui as a Key event and a press binding never fires.
const RELEASE_BINDINGS: &[(Modifiers, Key, Action)] =
    &[(Modifiers::CTRL, Key::V, Action::StopAfterCurrent)];

/// Consumed before BINDINGS while the playlist has focus. Ctrl and Shift
/// rows first: an unset Shift is ignored when matching.
const PLAYLIST_BINDINGS: &[(Modifiers, Key, Action)] = &[
    (Modifiers::CTRL, Key::ArrowUp, Action::MoveSelected(-1)),
    (Modifiers::CTRL, Key::ArrowDown, Action::MoveSelected(1)),
    (Modifiers::SHIFT, Key::ArrowUp, Action::Cursor(Move::By(-1), true)),
    (Modifiers::SHIFT, Key::ArrowDown, Action::Cursor(Move::By(1), true)),
    (Modifiers::SHIFT, Key::PageUp, Action::Cursor(Move::Page(-1), true)),
    (Modifiers::SHIFT, Key::PageDown, Action::Cursor(Move::Page(1), true)),
    (Modifiers::SHIFT, Key::Home, Action::Cursor(Move::First, true)),
    (Modifiers::SHIFT, Key::End, Action::Cursor(Move::Last, true)),
    (Modifiers::NONE, Key::ArrowUp, Action::Cursor(Move::By(-1), false)),
    (Modifiers::NONE, Key::ArrowDown, Action::Cursor(Move::By(1), false)),
    (Modifiers::NONE, Key::PageUp, Action::Cursor(Move::Page(-1), false)),
    (Modifiers::NONE, Key::PageDown, Action::Cursor(Move::Page(1), false)),
    (Modifiers::NONE, Key::Home, Action::Cursor(Move::First, false)),
    (Modifiers::NONE, Key::End, Action::Cursor(Move::Last, false)),
];

/// Consumes matching key presses. Call only when no text field has focus.
/// With `playlist_focus`, the cursor keys in PLAYLIST_BINDINGS win over BINDINGS.
pub fn collect(ctx: &egui::Context, playlist_focus: bool) -> Vec<Action> {
    ctx.input_mut(|i| {
        let first: &[_] = if playlist_focus { PLAYLIST_BINDINGS } else { &[] };
        let mut actions: Vec<Action> = first
            .iter()
            .chain(BINDINGS)
            .filter(|(m, k, _)| i.consume_key(*m, *k))
            .map(|(_, _, a)| *a)
            .collect();
        for (m, k, a) in RELEASE_BINDINGS {
            i.events.retain(|e| match e {
                egui::Event::Key { key, pressed: false, modifiers, .. }
                    if key == k && modifiers.matches_logically(*m) =>
                {
                    actions.push(*a);
                    false
                }
                _ => true,
            });
        }
        actions
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(focus: bool, modifiers: Modifiers, key: Key) -> Vec<Action> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![
                egui::Event::ModifiersChanged(modifiers),
                egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
            ],
            ..Default::default()
        };
        let mut actions = Vec::new();
        ctx.run_ui(input, |ui| actions = collect(ui.ctx(), focus))
            .drop_without_applying_deltas();
        actions
    }

    #[test]
    fn alt_v_shows_video() {
        assert_eq!(press(false, Modifiers::ALT, Key::V), [Action::ShowVideo]);
        assert_eq!(press(true, Modifiers::ALT, Key::V), [Action::ShowVideo]);
        assert_eq!(press(false, Modifiers::NONE, Key::V), [Action::Stop]);
    }

    fn release(focus: bool, modifiers: Modifiers, key: Key) -> Vec<Action> {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![
                egui::Event::ModifiersChanged(modifiers),
                egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: false,
                    repeat: false,
                    modifiers,
                },
            ],
            ..Default::default()
        };
        let mut actions = Vec::new();
        ctx.run_ui(input, |ui| actions = collect(ui.ctx(), focus))
            .drop_without_applying_deltas();
        actions
    }

    #[test]
    fn stop_after_and_sleep_keys() {
        let (c, n) = (Modifiers::CTRL, Modifiers::NONE);
        assert_eq!(release(false, c, Key::V), [Action::StopAfterCurrent]);
        assert_eq!(release(true, c, Key::V), [Action::StopAfterCurrent]);
        assert!(press(false, c, Key::V).is_empty());
        assert_eq!(press(false, n, Key::V), [Action::Stop]);
        assert!(release(false, n, Key::V).is_empty());
        assert_eq!(press(false, c, Key::S), [Action::SleepTimer]);
        assert_eq!(press(false, n, Key::S), [Action::ToggleShuffle]);
    }

    #[test]
    fn ctrl_l_downloads() {
        assert_eq!(press(false, Modifiers::CTRL, Key::L), [Action::DownloadUrl]);
        assert_eq!(press(false, Modifiers::NONE, Key::L), [Action::AddFiles]);
    }

    #[test]
    fn keys_map_to_actions() {
        assert_eq!(press(false, Modifiers::NONE, Key::C), [Action::PauseToggle]);
        // Alt+L must not also count as a plain L: Alt is ignored when matching,
        // so the Alt binding has to come first and consume the key.
        assert_eq!(press(false, Modifiers::ALT, Key::L), [Action::ToggleLibrary]);
        assert_eq!(press(false, Modifiers::NONE, Key::L), [Action::AddFiles]);
        assert_eq!(press(false, Modifiers::CTRL, Key::P), [Action::Settings]);
    }

    #[test]
    fn alt_3_opens_file_info() {
        assert_eq!(press(false, Modifiers::ALT, Key::Num3), [Action::FileInfo]);
        assert_eq!(press(true, Modifiers::ALT, Key::Num3), [Action::FileInfo]);
    }

    #[test]
    fn q_enqueues() {
        assert_eq!(press(false, Modifiers::NONE, Key::Q), [Action::EnqueueSelected]);
        assert_eq!(press(true, Modifiers::NONE, Key::Q), [Action::EnqueueSelected]);
    }

    #[test]
    fn ctrl_t_toggles_time() {
        let c = Modifiers::CTRL;
        assert_eq!(press(false, c, Key::T), [Action::ToggleTimeDisplay]);
        assert_eq!(press(true, c, Key::T), [Action::ToggleTimeDisplay]);
    }

    #[test]
    fn ctrl_d_toggles_double_size() {
        let c = Modifiers::CTRL;
        assert_eq!(press(false, c, Key::D), [Action::ToggleDoubleSize]);
        assert_eq!(press(true, c, Key::D), [Action::ToggleDoubleSize]);
        assert!(press(false, Modifiers::NONE, Key::D).is_empty());
    }

    #[test]
    fn playlist_focus_takes_arrows() {
        let n = Modifiers::NONE;
        assert_eq!(press(false, n, Key::ArrowUp), [Action::VolumeDelta(0.02)]);
        assert_eq!(
            press(true, n, Key::ArrowUp),
            [Action::Cursor(Move::By(-1), false)]
        );
        assert_eq!(
            press(true, Modifiers::SHIFT, Key::ArrowDown),
            [Action::Cursor(Move::By(1), true)]
        );
        assert_eq!(
            press(true, Modifiers::CTRL, Key::ArrowUp),
            [Action::MoveSelected(-1)]
        );
        assert_eq!(
            press(true, n, Key::PageDown),
            [Action::Cursor(Move::Page(1), false)]
        );
        assert_eq!(press(true, n, Key::ArrowLeft), [Action::SeekRel(-5000)]);
        assert!(press(false, n, Key::Home).is_empty());
        assert!(press(false, Modifiers::CTRL, Key::ArrowUp).is_empty());
    }
}
