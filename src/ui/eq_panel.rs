//! 10-band equalizer window.

use egui::{Align2, FontId, Rect, Slider, Stroke, Ui, pos2, vec2};

use super::widgets;
use crate::app::{App, Prompt, PromptKind};
use crate::audio::dsp::BAND_LABELS;
use crate::config::BANDS;
use crate::eq_presets::{MAX_DB, Preset};

pub fn show(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    let r = ui.max_rect().shrink(8.0);

    // --- header row: ON / AUTO / presets ------------------------------------
    let header = Rect::from_min_size(r.min, vec2(r.width(), 20.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(header)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            if widgets::led_toggle(ui, app.cfg.eq.enabled, "ON", &theme).clicked() {
                app.cfg.eq.enabled = !app.cfg.eq.enabled;
                app.push_eq();
                app.mark_cfg();
            }
            if widgets::led_toggle(ui, app.cfg.eq.auto, "AUTO", &theme)
                .on_hover_text("Remember the chosen preset per track")
                .clicked()
            {
                app.cfg.eq.auto = !app.cfg.eq.auto;
                app.mark_cfg();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let user = app.user_presets.iter().any(|p| p.name == app.cfg.eq.preset);
                if ui
                    .add_enabled(user, egui::Button::new("Delete"))
                    .on_hover_text("Delete this user preset")
                    .clicked()
                {
                    let name = app.cfg.eq.preset.clone();
                    app.delete_user_preset(&name);
                }
                if ui.button("Save…").clicked() {
                    app.prompt = Some(Prompt {
                        kind: PromptKind::SavePreset,
                        text: app.cfg.eq.preset.clone(),
                        focus: true,
                    });
                }
                if ui.button("Reset").clicked() {
                    app.apply_preset(&Preset {
                        name: "Flat".into(),
                        preamp: 0.0,
                        bands: [0.0; BANDS],
                    });
                }
                let mut chosen: Option<Preset> = None;
                let label = if app.cfg.eq.preset.is_empty() {
                    "Presets".to_string()
                } else {
                    app.cfg.eq.preset.clone()
                };
                egui::ComboBox::from_id_salt("eq-presets")
                    .selected_text(label)
                    .width(170.0)
                    .height(400.0)
                    .show_ui(ui, |ui| {
                        if !app.user_presets.is_empty() {
                            ui.label(egui::RichText::new("User").color(theme.text_dim));
                            for p in &app.user_presets {
                                if ui.selectable_label(app.cfg.eq.preset == p.name, &p.name).clicked() {
                                    chosen = Some(p.clone());
                                }
                            }
                            ui.separator();
                        }
                        for p in &app.builtin_presets {
                            if ui.selectable_label(app.cfg.eq.preset == p.name, &p.name).clicked() {
                                chosen = Some(p.clone());
                            }
                        }
                    });
                if let Some(p) = chosen {
                    app.apply_preset(&p);
                }
            });
        },
    );

    // --- sliders ------------------------------------------------------------
    let body = Rect::from_min_max(pos2(r.left(), header.bottom() + 8.0), r.max);
    let slider_h = body.height() - 18.0;
    let col_w = 40.0;
    let mut changed = false;

    // Response curve behind the band sliders.
    let bands_left = body.left() + col_w + 24.0;
    let bands_right = (bands_left + col_w * BANDS as f32).min(body.right());
    let curve_r = Rect::from_min_max(pos2(bands_left, body.top()), pos2(bands_right, body.top() + slider_h));
    widgets::lcd(ui, curve_r, &theme);
    {
        let p = ui.painter().with_clip_rect(curve_r);
        let mid = curve_r.center().y;
        p.hline(curve_r.x_range(), mid, Stroke::new(1.0, theme.frame));
        let y_of = |db: f32| mid - db / MAX_DB * (slider_h / 2.0 - 8.0);
        let pts: Vec<egui::Pos2> = (0..BANDS)
            .map(|i| pos2(bands_left + col_w * (i as f32 + 0.5), y_of(app.cfg.eq.bands[i])))
            .collect();
        let color = if app.cfg.eq.enabled { theme.accent } else { theme.muted };
        p.line(pts, Stroke::new(1.5, color.gamma_multiply(0.6)));
    }

    let slider_at = |ui: &mut Ui, x: f32, value: &mut f32, label: &str, color| -> bool {
        let col = Rect::from_min_size(pos2(x, body.top()), vec2(col_w, body.height()));
        let mut changed = false;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(col)
                .layout(egui::Layout::top_down(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().slider_width = slider_h - 8.0;
                ui.spacing_mut().item_spacing.y = 2.0;
                let resp = ui.add(
                    Slider::new(value, -MAX_DB..=MAX_DB)
                        .vertical()
                        .show_value(false)
                        .trailing_fill(false),
                );
                let resp = resp.on_hover_text(format!("{label}: {value:+.1} dB"));
                if resp.double_clicked() {
                    *value = 0.0;
                    changed = true;
                }
                changed |= resp.changed();
            },
        );
        ui.painter().text(
            pos2(x + col_w / 2.0, body.bottom() - 6.0),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(10.0),
            color,
        );
        changed
    };

    let mut preamp = app.cfg.eq.preamp;
    if slider_at(ui, body.left(), &mut preamp, "PRE", theme.text_bright) {
        app.cfg.eq.preamp = preamp;
        changed = true;
    }
    let mut bands = app.cfg.eq.bands;
    for i in 0..BANDS {
        let x = bands_left + col_w * i as f32;
        if slider_at(ui, x, &mut bands[i], BAND_LABELS[i], theme.text) {
            changed = true;
        }
    }
    if changed {
        app.cfg.eq.bands = bands;
        app.cfg.eq.preset.clear();
        app.push_eq();
        app.mark_cfg();
    }

    // dB scale labels.
    let scale_x = bands_left - 4.0;
    for (db, y) in [
        ("+12", curve_r.top() + 8.0),
        ("0", curve_r.center().y),
        ("-12", curve_r.bottom() - 8.0),
    ] {
        widgets::small_label(ui, pos2(scale_x, y), Align2::RIGHT_CENTER, db, theme.text_dim);
    }
}
