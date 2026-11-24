//! Settings window (Ctrl+P): a section list (Library, Playback, Audio, Appearance) and
//! one function per section.

use std::path::{Path, PathBuf};

use egui::{Id, Modal, RichText};

use crate::app::App;
use crate::config::Visualization;
use crate::dialog::Purpose;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Library,
    Playback,
    Audio,
    Appearance,
}

const SECTIONS: [(Section, &str); 4] = [
    (Section::Library, "Library"),
    (Section::Playback, "Playback"),
    (Section::Audio, "Audio"),
    (Section::Appearance, "Appearance"),
];

fn font_id() -> Id {
    Id::new("settings-font")
}

fn devices_id() -> Id {
    Id::new("settings-devices")
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    let section_id = Id::new("settings-section");
    let mut section = ctx
        .data(|d| d.get_temp::<Section>(section_id))
        .unwrap_or(Section::Library);
    let mut close = false;

    let resp = Modal::new(Id::new("settings")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(RichText::new("Settings").strong());
        ui.separator();

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                for (s, label) in SECTIONS {
                    if ui.selectable_label(section == s, label).clicked() {
                        section = s;
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| match section {
                Section::Library => library(app, ui),
                Section::Playback => playback(app, ui),
                Section::Audio => audio(app, ui),
                Section::Appearance => appearance(app, ui),
            });
        });

        ui.separator();
        if ui.button("Close").clicked() {
            close = true;
        }
    });

    ctx.data_mut(|d| d.insert_temp(section_id, section));
    if close || resp.should_close() {
        app.settings_open = false;
        ctx.data_mut(|d| {
            d.remove::<String>(font_id());
            d.remove::<Vec<(String, String)>>(devices_id());
        });
    }
}

fn library(app: &mut App, ui: &mut egui::Ui) {
    let scanning = app.scan_progress.is_some();
    let mut remove: Option<usize> = None;
    let mut add_folder = false;
    let mut rescan = false;
    let mut clear_covers = false;

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
        if ui.button("Clear cover cache").clicked() {
            clear_covers = true;
        }
    });

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
    if clear_covers {
        match crate::library::art::clear_cache() {
            Ok(()) => app.toast("Cover cache cleared"),
            Err(e) => app.toast(format!("Clear cover cache: {e}")),
        }
    }
}

fn playback(app: &mut App, ui: &mut egui::Ui) {
    if ui.checkbox(&mut app.cfg.gapless, "Gapless playback").changed() {
        app.mark_cfg();
        app.refresh_prefetch();
    }
    let mut video_on = app.cfg.video.enabled;
    if ui.checkbox(&mut video_on, "Play video in a separate window").changed() {
        app.set_video_enabled(video_on);
    }
    let video_hint = match app.video_error() {
        Some(e) => format!("Video unavailable: {e}. Video files play as audio."),
        None => "Alt+V raises the video window.".to_string(),
    };
    ui.label(RichText::new(video_hint).color(app.theme.text_dim));
    let mut toggle = false;
    ui.horizontal(|ui| {
        ui.label("Sleep timer");
        let mut m = app.cfg.sleep_minutes;
        if ui
            .add(egui::DragValue::new(&mut m).range(1..=480).suffix(" min"))
            .changed()
        {
            app.cfg.sleep_minutes = m;
            app.mark_cfg();
        }
        let label = if app.sleep_running() { "Cancel" } else { "Start" };
        toggle = ui.button(label).clicked();
    });
    ui.label(
        RichText::new("Ctrl+S starts or cancels. Fades out over 10 s, then pauses.")
            .color(app.theme.text_dim),
    );
    if toggle {
        app.toggle_sleep();
    }
    if ui
        .checkbox(&mut app.cfg.notify_track_change, "Notify on track change")
        .changed()
    {
        app.mark_cfg();
    }
    ui.label(RichText::new("Only while the window is unfocused.").color(app.theme.text_dim));
}

fn audio(app: &mut App, ui: &mut egui::Ui) {
    use crate::config::ReplayGainMode as M;
    ui.label(RichText::new("ReplayGain").strong());
    let mut mode = app.cfg.replaygain.mode;
    ui.horizontal(|ui| {
        ui.label("Mode");
        let shown = match mode {
            M::Off => "Off",
            M::Track => "Track",
            M::Album => "Album",
            M::Auto => "Auto (album; track when shuffled)",
        };
        egui::ComboBox::from_id_salt("rg-mode")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut mode, M::Off, "Off");
                ui.selectable_value(&mut mode, M::Track, "Track");
                ui.selectable_value(&mut mode, M::Album, "Album");
                ui.selectable_value(&mut mode, M::Auto, "Auto (album; track when shuffled)");
            });
    });
    if mode != app.cfg.replaygain.mode {
        app.cfg.replaygain.mode = mode;
        app.mark_cfg();
        app.publish_replay_gain();
    }
    ui.add_enabled_ui(mode != M::Off, |ui| {
        let mut preamp = app.cfg.replaygain.preamp_db;
        let mut untagged = app.cfg.replaygain.untagged_db;
        let mut changed = false;
        for (label, v) in [("Preamp", &mut preamp), ("Untagged tracks", &mut untagged)] {
            ui.horizontal(|ui| {
                ui.label(label);
                changed |= ui
                    .add(egui::Slider::new(v, -15.0..=15.0).step_by(0.5).suffix(" dB"))
                    .changed();
            });
        }
        if changed {
            app.cfg.replaygain.preamp_db = preamp;
            app.cfg.replaygain.untagged_db = untagged;
            app.mark_cfg();
            app.publish_replay_gain();
        }
    });
    ui.label(RichText::new("Peaks are never clipped.").color(app.theme.text_dim));
    ui.separator();
    // Listing makes a new sound-server client: once per settings session, not per frame.
    let cached = ui
        .ctx()
        .data_mut(|d| d.get_temp::<Vec<(String, String)>>(devices_id()));
    let devices = cached.unwrap_or_else(|| {
        let list = crate::audio::output::list_devices();
        ui.ctx()
            .data_mut(|d| d.insert_temp(devices_id(), list.clone()));
        list
    });
    let mut sel = app.cfg.output_device.clone();
    ui.horizontal(|ui| {
        ui.label("Output device");
        let shown = match &sel {
            None => "System default".to_owned(),
            Some(id) => devices
                .iter()
                .find(|(i, _)| i == id)
                .map(|(_, desc)| desc.clone())
                .unwrap_or_else(|| format!("{id} (not available)")),
        };
        egui::ComboBox::from_id_salt("output-device")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut sel, None, "System default");
                for (id, desc) in &devices {
                    ui.selectable_value(&mut sel, Some(id.clone()), desc);
                }
            });
        if ui.small_button("Refresh").clicked() {
            ui.ctx()
                .data_mut(|d| d.remove::<Vec<(String, String)>>(devices_id()));
        }
    });
    if sel != app.cfg.output_device {
        app.cfg.output_device = sel;
        app.mark_cfg();
        app.switch_output();
    }
}

fn appearance(app: &mut App, ui: &mut egui::Ui) {
    let dim = app.theme.text_dim;
    let mut follow = app.cfg.follow_omarchy_theme;
    let mut font_text: String = ui.ctx().data_mut(|d| {
        d.get_temp_mut_or_insert_with(font_id(), || {
            app.cfg
                .font_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        })
        .clone()
    });

    if ui.checkbox(&mut follow, "Follow Omarchy theme").changed() {
        app.set_follow_theme(follow);
    }
    if ui.checkbox(&mut app.cfg.double_size, "Double size (Ctrl+D)").changed() {
        app.mark_cfg();
    }
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
    if ui.checkbox(&mut app.cfg.show_cover, "Show cover").changed() {
        app.mark_cfg();
    }
    ui.horizontal(|ui| {
        ui.label("Visualization");
        let mut v = app.cfg.visualization;
        ui.selectable_value(&mut v, Visualization::Bars, "Bars");
        ui.selectable_value(&mut v, Visualization::Scope, "Scope");
        ui.selectable_value(&mut v, Visualization::Off, "Off");
        if v != app.cfg.visualization {
            app.cfg.visualization = v;
            app.mark_cfg();
        }
    });
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
    let check_id = Id::new("settings-font-check");
    let text = font_text.trim();
    let cached: Option<(String, Option<String>)> = ui.ctx().data(|d| d.get_temp(check_id));
    let rejected = match cached {
        Some((t, r)) if t == text => r,
        _ => {
            let r = (!text.is_empty())
                .then(|| crate::fonts::rejection(Path::new(text)))
                .flatten();
            ui.ctx()
                .data_mut(|d| d.insert_temp(check_id, (text.to_owned(), r.clone())));
            r
        }
    };
    if let Some(msg) = rejected {
        ui.label(RichText::new(msg).color(app.theme.warn));
    }
    ui.ctx().data_mut(|d| d.insert_temp(font_id(), font_text));
}
