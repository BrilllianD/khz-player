//! Settings window (Ctrl+P): library folders, theme, animation rate, time display, font.

use std::path::PathBuf;

use egui::{Id, Modal, RichText};

use crate::app::App;
use crate::dialog::Purpose;

pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    let scanning = app.scan_progress.is_some();
    let mut remove: Option<usize> = None;
    let mut add_folder = false;
    let mut rescan = false;
    let mut close = false;
    let mut follow = app.cfg.follow_omarchy_theme;
    let font_id = Id::new("settings-font");
    let mut font_text: String = ctx.data_mut(|d| {
        d.get_temp_mut_or_insert_with(font_id, || {
            app.cfg
                .font_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        })
        .clone()
    });
    let dim = app.theme.text_dim;

    let resp = Modal::new(Id::new("settings")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.label(RichText::new("Settings").strong());
        ui.separator();

        ui.label(RichText::new("Library folders").strong());
        for (i, root) in app.cfg.library_roots.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button("Remove").clicked() {
                    remove = Some(i);
                }
                ui.label(root.display().to_string());
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Add folder…").clicked() {
                add_folder = true;
            }
            if ui
                .add_enabled(!scanning, egui::Button::new("Rescan now"))
                .clicked()
            {
                rescan = true;
            }
        });
        ui.separator();

        ui.checkbox(&mut follow, "Follow Omarchy theme");
        ui.horizontal(|ui| {
            ui.label("Animation frame rate");
            let mut fps = app.cfg.animation_fps;
            if ui.add(egui::Slider::new(&mut fps, 10..=60)).changed() {
                app.cfg.animation_fps = fps;
                app.mark_cfg();
            }
        });
        if ui
            .checkbox(&mut app.cfg.time_remaining, "Show remaining time")
            .changed()
        {
            app.mark_cfg();
        }
        ui.horizontal(|ui| {
            ui.label("Font file");
            ui.label(RichText::new("applies after restart").color(dim));
        });
        let te = ui.add(
            egui::TextEdit::singleline(&mut font_text)
                .hint_text("empty = automatic Nerd Font")
                .desired_width(f32::INFINITY),
        );
        if te.changed() {
            let t = font_text.trim();
            app.cfg.font_path = (!t.is_empty()).then(|| PathBuf::from(t));
            app.mark_cfg();
        }
        ui.separator();
        if ui.button("Close").clicked() {
            close = true;
        }
    });

    ctx.data_mut(|d| d.insert_temp(font_id, font_text));
    if let Some(i) = remove {
        app.cfg.library_roots.remove(i);
        app.mark_cfg();
    }
    if add_folder {
        app.open_dialog(Purpose::LibraryRoot);
    }
    if rescan {
        app.rescan();
    }
    if follow != app.cfg.follow_omarchy_theme {
        app.set_follow_theme(follow);
    }
    if close || resp.should_close() {
        app.settings_open = false;
        ctx.data_mut(|d| d.remove::<String>(font_id));
    }
}
