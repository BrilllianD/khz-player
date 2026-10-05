//! Main window: title strip, time, marquee, spectrum, sliders, transport.

use std::time::Duration;

use egui::{Align2, FontId, Id, Rect, Sense, Ui, pos2, vec2};

use super::widgets;
use crate::app::App;
use crate::audio::{Command, PlayerState};
use crate::library::format_duration;
use crate::shortcuts::Action;

const PAD: f32 = 8.0;
const TITLE_H: f32 = 20.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let r = ui.max_rect();
    let theme = app.theme.clone();
    let icons = super::icons(app.nerd_font);
    let ctx = ui.ctx().clone();
    let x0 = r.left() + PAD;
    let x1 = r.right() - PAD;

    // --- title strip -------------------------------------------------------
    let title = Rect::from_min_max(r.min, pos2(r.right(), r.top() + TITLE_H));
    let drag = ui.interact(title, Id::new("title-drag"), Sense::click_and_drag());
    if drag.drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    {
        let p = ui.painter();
        p.rect_filled(title, 0.0, theme.panel);
        p.hline(title.x_range(), title.bottom(), egui::Stroke::new(1.0, theme.frame));
        p.text(
            pos2(x0, title.center().y),
            Align2::LEFT_CENTER,
            format!("{}  kHz", icons.music),
            FontId::monospace(12.0),
            theme.text_bright,
        );
    }
    let bsz = vec2(18.0, 14.0);
    let close_r = Rect::from_center_size(pos2(title.right() - 14.0, title.center().y), bsz);
    let min_r = Rect::from_center_size(pos2(title.right() - 36.0, title.center().y), bsz);
    if title_button(ui, close_r, "x", &theme).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    if title_button(ui, min_r, "_", &theme).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }

    // --- time / marquee / spectrum -----------------------------------------
    let top = title.bottom() + 6.0;
    let digits_r = Rect::from_min_size(pos2(x0, top), vec2(180.0, 60.0));
    let pos = app.audio.position();
    let dur = app.duration();
    let shown = match app.seek_drag {
        Some(f) => dur.map(|d| d.mul_f32(f)).unwrap_or(pos),
        None => pos,
    };
    let time_text = if app.now.is_none() || app.state == PlayerState::Stopped {
        "  :  ".to_string()
    } else if app.cfg.time_remaining
        && let Some(d) = dur
    {
        format!("-{}", format_duration(d.saturating_sub(shown)))
    } else {
        format_duration(shown)
    };
    if widgets::digits(ui, digits_r, &time_text, &theme).clicked() {
        app.cfg.time_remaining = !app.cfg.time_remaining;
        app.mark_cfg();
    }
    // Play state glyph in the LCD corner; blinks while paused like Winamp.
    let glyph = match app.state {
        PlayerState::Playing => icons.play,
        PlayerState::Paused => icons.pause,
        PlayerState::Stopped => icons.stop,
    };
    let blink_off = app.state == PlayerState::Paused
        && (ui.input(|i| i.time) * 2.0) as i64 % 2 == 1;
    if !blink_off {
        ui.painter().text(
            digits_r.left_top() + vec2(8.0, 6.0),
            Align2::LEFT_TOP,
            glyph,
            FontId::proportional(12.0),
            theme.accent,
        );
    }
    if app.state == PlayerState::Paused {
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    // Info line under the digits.
    let info_y = digits_r.bottom() + 12.0;
    let (kbps, khz, stereo) = match (&app.now, &app.now_track) {
        (Some(n), t) => (
            n.kbps.or(t.as_ref().and_then(|t| t.bitrate)),
            Some(n.sample_rate),
            n.channels >= 2,
        ),
        _ => (None, None, true),
    };
    let kb = kbps.map_or("---".into(), |k| k.to_string());
    let kh = khz.map_or("--".into(), |r| format!("{}", (r as f32 / 1000.0).round()));
    widgets::small_label(ui, pos2(x0, info_y), Align2::LEFT_CENTER, &format!("{kb:>4} kbps"), theme.text);
    widgets::small_label(ui, pos2(x0 + 82.0, info_y), Align2::LEFT_CENTER, &format!("{kh} kHz"), theme.text);
    let loaded = app.now.is_some();
    widgets::small_label(
        ui,
        pos2(x0 + 138.0, info_y),
        Align2::LEFT_CENTER,
        if stereo { "stereo" } else { "mono" },
        if loaded { theme.accent } else { theme.muted },
    );

    let right_x = digits_r.right() + 8.0;
    let marquee_r = Rect::from_min_max(pos2(right_x, top), pos2(x1, top + 20.0));
    let title_text = marquee_text(app);
    // Scrolls only while playing (App::logic already repaints at animation_fps then);
    // paused/stopped it holds at the start.
    let playing = app.state == PlayerState::Playing;
    let marquee_t = if playing { app.marquee_start.elapsed().as_secs_f32() } else { 0.0 };
    widgets::marquee(
        ui,
        marquee_r,
        &title_text,
        marquee_t,
        &theme,
    );
    let spec_r = Rect::from_min_max(pos2(right_x, marquee_r.bottom() + 4.0), pos2(x1, top + 76.0));
    widgets::spectrum(ui, spec_r, &app.spectrum.bars, &app.spectrum.peaks, &theme);

    // --- volume / balance / panel toggles ----------------------------------
    let row_y = top + 84.0;
    // Bars shrink when the window is narrower than MAIN_W (tiled), so BAL
    // never runs under the panel toggles on the right.
    let toggles_x = x1 - 140.0;
    let s = ((toggles_x - 8.0 - x0 - 66.0) / 260.0).clamp(0.3, 1.0);
    let (vol_w, bal_w) = (170.0 * s, 90.0 * s);
    let bal_x = x0 + 28.0 + vol_w + 10.0;
    widgets::small_label(ui, pos2(x0, row_y + 8.0), Align2::LEFT_CENTER, "VOL", theme.text_dim);
    let vol_r = Rect::from_min_size(pos2(x0 + 28.0, row_y), vec2(vol_w, 16.0));
    let (vresp, v) = widgets::bar(ui, vol_r, app.cfg.volume, false, &theme);
    if let Some(v) = v {
        app.set_volume(v);
    }
    // 2 % per wheel notch. Read the raw events: the smoothed scroll delta
    // spreads one notch over several frames.
    let notches = if vresp.hovered() { ui.input(wheel_notches) } else { 0.0 };
    if notches != 0.0 {
        app.set_volume(app.cfg.volume + notches * 0.02);
    }
    vresp.on_hover_text(format!("Volume {:.0}%", app.cfg.volume * 100.0));

    widgets::small_label(ui, pos2(bal_x, row_y + 8.0), Align2::LEFT_CENTER, "BAL", theme.text_dim);
    let bal_r = Rect::from_min_size(pos2(bal_x + 28.0, row_y), vec2(bal_w, 16.0));
    let (bresp, b) = widgets::bar(ui, bal_r, (app.cfg.balance + 1.0) / 2.0, true, &theme);
    if let Some(b) = b {
        app.set_balance(b * 2.0 - 1.0);
    }
    if bresp.double_clicked() {
        app.set_balance(0.0);
    }
    let bal_text = match app.cfg.balance {
        b if b < 0.0 => format!("Balance {:.0}% left", -b * 100.0),
        b if b > 0.0 => format!("Balance {:.0}% right", b * 100.0),
        _ => "Balance center".into(),
    };
    bresp.on_hover_text(bal_text);

    let toggles = Rect::from_min_max(pos2(toggles_x, row_y - 1.0), pos2(x1, row_y + 17.0));
    let mut toggle_actions = Vec::new();
    ui.scope_builder(egui::UiBuilder::new().max_rect(toggles).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        if widgets::led_toggle(ui, app.cfg.show_library, "LIB", &theme).clicked() {
            toggle_actions.push(Action::ToggleLibrary);
        }
        if widgets::led_toggle(ui, app.cfg.show_playlist, "PL", &theme).clicked() {
            toggle_actions.push(Action::TogglePlaylist);
        }
        if widgets::led_toggle(ui, app.cfg.show_eq, "EQ", &theme).clicked() {
            toggle_actions.push(Action::ToggleEq);
        }
    });

    // --- seek bar ----------------------------------------------------------
    let seek_y = row_y + 26.0;
    let seek_r = Rect::from_min_max(pos2(x0, seek_y), pos2(x1, seek_y + 14.0));
    let frac = match (app.seek_drag, dur) {
        (Some(f), _) => f,
        (None, Some(d)) if !d.is_zero() && app.state != PlayerState::Stopped => {
            (pos.as_secs_f32() / d.as_secs_f32()).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    let (sresp, s) = widgets::bar(ui, seek_r, frac, false, &theme);
    if dur.is_some() && app.now.is_some() {
        if let Some(f) = s {
            app.seek_drag = Some(f);
        }
        if (sresp.drag_stopped() || (sresp.clicked() && !sresp.dragged()))
            && let (Some(f), Some(d)) = (app.seek_drag.take(), dur)
        {
            app.audio.send(Command::Seek(d.mul_f32(f)));
        }
    } else {
        app.seek_drag = None;
    }

    // --- transport ---------------------------------------------------------
    let tr_y = seek_y + 22.0;
    let tr = Rect::from_min_max(pos2(x0, tr_y), pos2(x1, tr_y + 24.0));
    let mut actions = Vec::new();
    ui.scope_builder(egui::UiBuilder::new().max_rect(tr).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let sz = vec2(34.0, 22.0);
        if widgets::button(ui, sz, icons.prev, false, &theme).on_hover_text("Previous (Z)").clicked() {
            actions.push(Action::Prev);
        }
        if widgets::button(ui, sz, icons.play, app.state == PlayerState::Playing, &theme).on_hover_text("Play (X)").clicked() {
            actions.push(Action::Play);
        }
        if widgets::button(ui, sz, icons.pause, app.state == PlayerState::Paused, &theme).on_hover_text("Pause (C)").clicked() {
            actions.push(Action::TogglePlay);
        }
        if widgets::button(ui, sz, icons.stop, false, &theme).on_hover_text("Stop (V)").clicked() {
            actions.push(Action::Stop);
        }
        if widgets::button(ui, sz, icons.next, false, &theme).on_hover_text("Next (B)").clicked() {
            actions.push(Action::Next);
        }
        ui.add_space(6.0);
        if widgets::button(ui, vec2(28.0, 22.0), icons.eject, false, &theme).on_hover_text("Open (L)").clicked() {
            actions.push(Action::FocusOpen);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let rep = app.cfg.repeat;
            if widgets::led_toggle(ui, rep != crate::playlist::Repeat::Off, &format!("{} {}", icons.repeat, rep.label()), &theme)
                .on_hover_text("Repeat: off / all / one (R)")
                .clicked()
            {
                actions.push(Action::CycleRepeat);
            }
            if widgets::led_toggle(ui, app.cfg.shuffle, &format!("{} SHUF", icons.shuffle), &theme)
                .on_hover_text("Shuffle (S)")
                .clicked()
            {
                actions.push(Action::ToggleShuffle);
            }
        });
    });

    // --- open path field -----------------------------------------------------
    let open_y = tr_y + 30.0;
    let open_r = Rect::from_min_max(pos2(x0, open_y), pos2(x1, open_y + 20.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(open_r).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        let te = ui.add(
            egui::TextEdit::singleline(&mut app.open_path)
                .hint_text("Open file / folder / .m3u — Enter adds, Shift+Enter adds & plays")
                .font(FontId::monospace(11.0))
                .desired_width(f32::INFINITY)
                .id(Id::new("open-path")),
        );
        if app.focus_open {
            te.request_focus();
            app.focus_open = false;
        }
        if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let text = app.open_path.trim().to_string();
            if !text.is_empty() {
                let play = ui.input(|i| i.modifiers.shift);
                let list = app.active;
                let before = app.playlists[list].len();
                app.add_path(std::path::Path::new(&text));
                if play && app.playlists[list].len() > before {
                    app.play_index(list, before);
                }
                app.open_path.clear();
            }
        }
    });

    for a in toggle_actions.into_iter().chain(actions) {
        app.dispatch(&ctx, a);
    }
}

fn title_button(ui: &mut Ui, rect: Rect, text: &str, theme: &crate::theme::Theme) -> egui::Response {
    let resp = ui.interact(rect, Id::new(("title-btn", text)), Sense::click());
    let p = ui.painter();
    let bg = if resp.hovered() { theme.selection } else { theme.bg_dark };
    p.rect_filled(rect, 0.0, bg);
    p.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, theme.frame), egui::StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, text, FontId::monospace(10.0), theme.text_bright);
    resp
}

fn marquee_text(app: &App) -> String {
    let Some(t) = &app.now_track else {
        return "khz-player — drop files here or press L to open".into();
    };
    let idx = app
        .playlists
        .get(app.playing_list)
        .and_then(|p| p.current)
        .map(|i| format!("{}. ", i + 1))
        .unwrap_or_default();
    let dur = app
        .duration()
        .map(|d| format!(" ({})", format_duration(d)))
        .unwrap_or_default();
    format!("{idx}{}{dur}", t.display_name())
}

/// Vertical wheel movement this frame in notches; touchpad pixels count
/// 50 to a notch.
fn wheel_notches(i: &egui::InputState) -> f32 {
    i.events
        .iter()
        .map(|e| match e {
            egui::Event::MouseWheel { unit, delta, .. } => match unit {
                egui::MouseWheelUnit::Line => delta.y,
                egui::MouseWheelUnit::Point => delta.y / 50.0,
                egui::MouseWheelUnit::Page => delta.y * 10.0,
            },
            _ => 0.0,
        })
        .sum()
}
