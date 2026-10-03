pub mod eq_panel;
pub mod jump_dialog;
pub mod library_panel;
pub mod main_panel;
pub mod playlist_panel;
pub mod widgets;

use egui::{Align2, Frame, Id, Margin, Modal, Panel, RichText};

use crate::app::{App, PromptKind};

pub const MAIN_W: f32 = 560.0;
pub const MAIN_H: f32 = 232.0;
pub const EQ_H: f32 = 200.0;
pub const PLAYLIST_H: f32 = 330.0;
pub const PLAYLIST_MIN_H: f32 = 200.0;
pub const LIBRARY_W: f32 = 420.0;

/// Icon glyphs: Font Awesome codepoints from the Nerd Font when present,
/// plain Unicode otherwise.
pub struct Icons {
    pub prev: &'static str,
    pub play: &'static str,
    pub pause: &'static str,
    pub stop: &'static str,
    pub next: &'static str,
    pub eject: &'static str,
    pub shuffle: &'static str,
    pub repeat: &'static str,
    pub music: &'static str,
}

pub fn icons(nerd: bool) -> Icons {
    if nerd {
        Icons {
            prev: "\u{f048}",
            play: "\u{f04b}",
            pause: "\u{f04c}",
            stop: "\u{f04d}",
            next: "\u{f051}",
            eject: "\u{f052}",
            shuffle: "\u{f074}",
            repeat: "\u{f01e}",
            music: "\u{f001}",
        }
    } else {
        Icons {
            prev: "⏮",
            play: "▶",
            pause: "⏸",
            stop: "⏹",
            next: "⏭",
            eject: "⏏",
            shuffle: "⤨",
            repeat: "⟳",
            music: "♪",
        }
    }
}

fn section_frame(app: &App) -> Frame {
    Frame::NONE
        .fill(app.theme.bg)
        .inner_margin(Margin::same(0))
        .stroke(egui::Stroke::new(1.0, app.theme.frame))
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();

    if app.cfg.show_library {
        Panel::right("library")
            .resizable(true)
            .default_size(LIBRARY_W)
            .min_size(260.0)
            .frame(section_frame(app).inner_margin(Margin::same(4)))
            .show(ui, |ui| library_panel::show(app, ui));
    }

    Panel::top("main")
        .exact_size(MAIN_H)
        .resizable(false)
        .show_separator_line(false)
        .frame(section_frame(app))
        .show(ui, |ui| main_panel::show(app, ui));

    if app.cfg.show_eq {
        Panel::top("eq")
            .exact_size(EQ_H)
            .resizable(false)
            .show_separator_line(false)
            .frame(section_frame(app))
            .show(ui, |ui| eq_panel::show(app, ui));
    }

    egui::CentralPanel::default()
        .frame(section_frame(app))
        .show(ui, |ui| {
            if app.cfg.show_playlist {
                playlist_panel::show(app, ui);
            }
        });

    prompt_modal(app, &ctx);
    jump_dialog::show(app, &ctx);
    toasts(app, &ctx);
    resize_grip(&ctx, &app.theme);
}

fn prompt_modal(app: &mut App, ctx: &egui::Context) {
    let Some(prompt) = &mut app.prompt else {
        return;
    };
    let title = match prompt.kind {
        PromptKind::AddPath => "Add file, folder or playlist",
        PromptKind::ImportM3u => "Import M3U as new playlist",
        PromptKind::ExportM3u => "Export playlist to M3U",
        PromptKind::NewPlaylist => "New playlist name",
        PromptKind::RenamePlaylist(_) => "Rename playlist",
        PromptKind::SavePreset => "Save EQ preset as",
    };
    let mut submit = false;
    let mut cancel = false;
    let resp = Modal::new(Id::new("prompt")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new(title).strong());
        let te = ui.add(
            egui::TextEdit::singleline(&mut prompt.text)
                .desired_width(f32::INFINITY)
                .id(Id::new("prompt-text")),
        );
        if prompt.focus {
            te.request_focus();
            prompt.focus = false;
        }
        if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submit = true;
        }
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() {
                submit = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if resp.should_close() {
        cancel = true;
    }
    if submit {
        let p = app.prompt.take().expect("prompt");
        let text = p.text.trim().to_string();
        if text.is_empty() {
            return;
        }
        match p.kind {
            PromptKind::AddPath => app.add_path(std::path::Path::new(&text)),
            PromptKind::ImportM3u => app.import_m3u_as_new(std::path::Path::new(&text)),
            PromptKind::ExportM3u => app.export_m3u(std::path::Path::new(&text)),
            PromptKind::NewPlaylist => {
                app.new_playlist(text);
            }
            PromptKind::RenamePlaylist(i) => app.rename_playlist(i, text),
            PromptKind::SavePreset => app.save_user_preset(text),
        }
    } else if cancel {
        app.prompt = None;
    }
}

fn toasts(app: &App, ctx: &egui::Context) {
    if app.toasts.is_empty() {
        return;
    }
    egui::Area::new(Id::new("toasts"))
        .anchor(Align2::LEFT_BOTTOM, [8.0, -8.0])
        .interactable(false)
        .show(ctx, |ui| {
            for (msg, _) in app.toasts.iter().rev().take(4) {
                Frame::NONE
                    .fill(app.theme.bg_darker)
                    .stroke(egui::Stroke::new(1.0, app.theme.warn))
                    .inner_margin(Margin::symmetric(6, 3))
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.label(RichText::new(msg).color(app.theme.text_bright));
                    });
            }
        });
}

/// Bottom-right corner handle for resizing the undecorated window.
fn resize_grip(ctx: &egui::Context, theme: &crate::theme::Theme) {
    let screen = ctx.content_rect();
    let size = 12.0;
    let rect = egui::Rect::from_min_size(screen.max - egui::vec2(size, size), egui::vec2(size, size));
    egui::Area::new(Id::new("resize-grip"))
        .fixed_pos(rect.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            let (resp, painter) = ui.allocate_painter(rect.size(), egui::Sense::drag());
            let c = if resp.hovered() { theme.accent } else { theme.frame };
            for k in 1..=3 {
                let o = k as f32 * 3.5;
                painter.line_segment(
                    [
                        egui::pos2(rect.max.x - o, rect.max.y - 1.0),
                        egui::pos2(rect.max.x - 1.0, rect.max.y - o),
                    ],
                    egui::Stroke::new(1.0, c),
                );
            }
            if resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            if resp.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(
                    egui::viewport::ResizeDirection::SouthEast,
                ));
            }
        });
}
