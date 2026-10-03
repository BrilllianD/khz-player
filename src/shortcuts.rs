//! Global keyboard shortcuts (Winamp layout).

use egui::{Key, Modifiers};

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
    FocusOpen,
    Jump,
    RemoveSelected,
    PlaySelected,
    SelectAll,
}

const BINDINGS: &[(Modifiers, Key, Action)] = &[
    (Modifiers::NONE, Key::Z, Action::Prev),
    (Modifiers::NONE, Key::X, Action::Play),
    (Modifiers::NONE, Key::C, Action::PauseToggle),
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
    (Modifiers::NONE, Key::L, Action::FocusOpen),
    (Modifiers::NONE, Key::J, Action::Jump),
    (Modifiers::NONE, Key::Delete, Action::RemoveSelected),
    (Modifiers::NONE, Key::Enter, Action::PlaySelected),
    (Modifiers::CTRL, Key::A, Action::SelectAll),
];

/// Consumes matching key presses. Call only when no text field has focus.
pub fn collect(ctx: &egui::Context) -> Vec<Action> {
    ctx.input_mut(|i| {
        BINDINGS
            .iter()
            .filter(|(m, k, _)| i.consume_key(*m, *k))
            .map(|(_, _, a)| *a)
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(modifiers: Modifiers, key: Key) -> Vec<Action> {
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
        ctx.run_ui(input, |ui| actions = collect(ui.ctx()))
            .drop_without_applying_deltas();
        actions
    }

    #[test]
    fn keys_map_to_actions() {
        assert_eq!(press(Modifiers::NONE, Key::C), [Action::PauseToggle]);
        // Alt+L must not also count as a plain L: Alt is ignored when matching,
        // so the Alt binding has to come first and consume the key.
        assert_eq!(press(Modifiers::ALT, Key::L), [Action::ToggleLibrary]);
        assert_eq!(press(Modifiers::NONE, Key::L), [Action::FocusOpen]);
    }
}
