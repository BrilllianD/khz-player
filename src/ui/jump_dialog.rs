//! "Jump to file" (J): filter the visible playlist and play the chosen track.

use egui::{Id, Key, Modal, RichText};

use crate::app::App;

const MAX_SHOWN: usize = 200;

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(jump) = &mut app.jump else {
        return;
    };
    let theme = app.theme.clone();
    let pl = &app.playlists[app.active];
    let terms: Vec<String> = jump
        .query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let hits: Vec<usize> = pl
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            let name = t.display_name().to_lowercase();
            terms.iter().all(|w| name.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .take(MAX_SHOWN)
        .collect();

    let (up, down, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
        )
    });
    if up {
        jump.cursor = jump.cursor.saturating_sub(1);
    }
    if down {
        jump.cursor = (jump.cursor + 1).min(hits.len().saturating_sub(1));
    }
    jump.cursor = jump.cursor.min(hits.len().saturating_sub(1));

    let mut chosen = if enter { hits.get(jump.cursor).copied() } else { None };
    let resp = Modal::new(Id::new("jump")).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.label(RichText::new("Jump to file").strong());
        let te = ui.add(
            egui::TextEdit::singleline(&mut jump.query)
                .hint_text("type to filter…")
                .desired_width(f32::INFINITY),
        );
        if jump.focus {
            te.request_focus();
            jump.focus = false;
        }
        if te.changed() {
            jump.cursor = 0;
        }
        egui::ScrollArea::vertical()
            .max_height(360.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for (n, &i) in hits.iter().enumerate() {
                    let t = &pl.tracks[i];
                    let sel = n == jump.cursor;
                    let r = ui.selectable_label(
                        sel,
                        RichText::new(format!("{}. {}", i + 1, t.display_name()))
                            .monospace()
                            .color(if sel { theme.text_bright } else { theme.text }),
                    );
                    if sel && (up || down) {
                        r.scroll_to_me(None);
                    }
                    if r.clicked() {
                        chosen = Some(i);
                    }
                }
            });
        ui.label(
            RichText::new("Enter play · Esc close · ↑↓ select")
                .small()
                .color(theme.text_dim),
        );
    });
    let close = resp.should_close();
    if let Some(i) = chosen {
        app.jump = None;
        app.play_index(app.active, i);
        app.scroll_to = Some(i);
    } else if close {
        app.jump = None;
    }
}
