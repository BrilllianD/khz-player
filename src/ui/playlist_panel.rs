//! Playlist editor: tabs, track list, ADD/REM/SEL/MISC/LIST menus.

use egui::{Align2, FontId, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::app::{App, Prompt, PromptKind};
use crate::audio::PlayerState;
use crate::library::format_duration;

const ROW_H: f32 = 17.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    let full = ui.max_rect().shrink(4.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(full), |ui| {
        tabs(app, ui);
        let footer_h = 24.0;
        let list_h = (ui.available_height() - footer_h).max(40.0);
        let list_rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), list_h));
        ui.painter()
            .rect_filled(list_rect, 0.0, theme.bg_darker);
        ui.scope_builder(egui::UiBuilder::new().max_rect(list_rect), |ui| {
            rows(app, ui);
        });
        ui.advance_cursor_after_rect(list_rect);
        footer(app, ui);
    });
}

fn tabs(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    let mut switch = None;
    let mut action: Option<PromptKind> = None;
    let mut delete = None;
    egui::ScrollArea::horizontal()
        .id_salt("pl-tabs")
        .max_height(22.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, p) in app.playlists.iter().enumerate() {
                    let playing = i == app.playing_list && app.state != PlayerState::Stopped;
                    let text = if playing {
                        RichText::new(format!("▸ {}", p.name))
                    } else {
                        RichText::new(&p.name)
                    };
                    let resp = ui.selectable_label(i == app.active, text.monospace());
                    if resp.clicked() {
                        switch = Some(i);
                    }
                    if resp.double_clicked() {
                        action = Some(PromptKind::RenamePlaylist(i));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Rename…").clicked() {
                            action = Some(PromptKind::RenamePlaylist(i));
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            delete = Some(i);
                            ui.close();
                        }
                    });
                }
                if ui
                    .button(RichText::new("+").monospace().color(theme.accent))
                    .on_hover_text("New playlist")
                    .clicked()
                {
                    action = Some(PromptKind::NewPlaylist);
                }
            });
        });
    if let Some(i) = switch {
        app.active = i;
        app.mark_cfg();
    }
    if let Some(i) = delete {
        app.delete_playlist(i);
    }
    if let Some(kind) = action {
        let text = match &kind {
            PromptKind::RenamePlaylist(i) => app.playlists[*i].name.clone(),
            _ => String::new(),
        };
        app.prompt = Some(Prompt {
            kind,
            text,
            focus: true,
        });
    }
}

fn rows(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    let active = app.active;
    let is_playing_list = active == app.playing_list && app.now.is_some();
    let n = app.playlists[active].len();
    if n == 0 {
        ui.painter().text(
            ui.max_rect().center(),
            Align2::CENTER_CENTER,
            "Empty — drop files, press L, or add from the library (Alt+L)",
            FontId::monospace(11.0),
            theme.text_dim,
        );
        return;
    }
    let num_w = format!("{n}").len() as f32 * 7.5 + 12.0;
    let font = FontId::monospace(12.0);
    let mut clicked: Option<(usize, egui::Modifiers)> = None;
    let mut play: Option<usize> = None;
    let mut remove_ctx = false;
    // Drag-reorder: set when a row drag starts, cleared on release.
    let drag_id = egui::Id::new(("pl-drag", active));
    let mut dragging = ui.ctx().data(|d| d.get_temp::<bool>(drag_id)).unwrap_or(false);
    // The release can happen where this panel never sees it (outside the
    // window, another tab, panel hidden); a flag with no button held, or with
    // a new press, is left over from such a drag.
    if dragging
        && ui.input(|i| {
            i.pointer.any_pressed() || !(i.pointer.any_down() || i.pointer.any_released())
        })
    {
        ui.ctx().data_mut(|d| d.remove::<bool>(drag_id));
        dragging = false;
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let mut drag_start: Option<usize> = None;
    let mut drop_gap: Option<usize> = None;

    let mut area = egui::ScrollArea::vertical()
        .id_salt(("pl-rows", active))
        .auto_shrink([false, false]);
    // Scroll offset and height of the list as last shown.
    let view_id = egui::Id::new(("pl-view", active));
    if let Some(row) = app.scroll_to.take() {
        let view = ui.ctx().data(|d| d.get_temp::<(f32, f32)>(view_id));
        let view = view.unwrap_or((f32::NAN, ui.available_height()));
        if let Some(y) = scroll_target(row, view) {
            area = area.vertical_scroll_offset(y);
        }
    }
    // show_rows takes its row stride from this ui's spacing, so zero it here,
    // not inside the closure, or rows drift from where egui puts them.
    ui.spacing_mut().item_spacing.y = 0.0;
    let out = area.show_rows(ui, ROW_H, n, |ui, range| {
        let pl = &app.playlists[active];
        for i in range {
            let t = &pl.tracks[i];
            let (rect, resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
            let selected = pl.selected.contains(&i);
            let current = is_playing_list && pl.current == Some(i);
            let p = ui.painter();
            if selected {
                p.rect_filled(rect, 0.0, theme.selection);
            } else if resp.hovered() {
                p.rect_filled(rect, 0.0, theme.bg_dark);
            }
            let color = if current {
                theme.text_bright
            } else if app.failed.contains(&t.path)
                || (t.title.is_none() && t.duration_ms.is_none() && !t.path.exists())
            {
                theme.muted
            } else {
                theme.text
            };
            let painter = p.with_clip_rect(rect);
            painter.text(
                pos2(rect.left() + num_w - 6.0, rect.center().y),
                Align2::RIGHT_CENTER,
                format!("{}.", i + 1),
                font.clone(),
                if current { theme.accent } else { theme.text_dim },
            );
            let dur = t.duration().map(format_duration).unwrap_or_default();
            let dur_rect = painter.text(
                pos2(rect.right() - 6.0, rect.center().y),
                Align2::RIGHT_CENTER,
                &dur,
                font.clone(),
                color,
            );
            let name_clip = Rect::from_min_max(
                pos2(rect.left() + num_w, rect.top()),
                pos2(dur_rect.left() - 8.0, rect.bottom()),
            );
            painter.with_clip_rect(name_clip).text(
                pos2(name_clip.left(), rect.center().y),
                Align2::LEFT_CENTER,
                t.display_name(),
                font.clone(),
                color,
            );
            if resp.drag_started() {
                drag_start = Some(i);
            }
            if dragging
                && let Some(pos) = pointer
                && (rect.top()..rect.bottom()).contains(&pos.y)
            {
                let gap = if pos.y < rect.center().y { i } else { i + 1 };
                let y = if gap == i { rect.top() } else { rect.bottom() };
                p.hline(rect.x_range(), y, egui::Stroke::new(2.0, theme.accent));
                drop_gap = Some(gap);
            }
            if resp.double_clicked() {
                play = Some(i);
            } else if resp.clicked() {
                clicked = Some((i, ui.input(|inp| inp.modifiers)));
            }
            if resp.secondary_clicked() && !selected {
                clicked = Some((i, egui::Modifiers::NONE));
            }
            resp.context_menu(|ui| {
                if ui.button("Play").clicked() {
                    play = Some(i);
                    ui.close();
                }
                if ui.button("Remove selected").clicked() {
                    remove_ctx = true;
                    ui.close();
                }
                ui.separator();
                ui.label(RichText::new(t.path.to_string_lossy()).small().color(theme.text_dim));
            });
        }
        // Scroll while dragging near the top or bottom edge.
        if dragging && let Some(pos) = pointer {
            let clip = ui.clip_rect();
            if pos.y < clip.top() + ROW_H {
                ui.scroll_with_delta(vec2(0.0, 6.0));
            } else if pos.y > clip.bottom() - ROW_H {
                ui.scroll_with_delta(vec2(0.0, -6.0));
            }
        }
    });
    let view = (out.state.offset.y, out.inner_rect.height());
    ui.ctx().data_mut(|d| d.insert_temp(view_id, view));

    if let Some(i) = drag_start {
        let pl = &mut app.playlists[active];
        if !pl.selected.contains(&i) {
            pl.selected.clear();
            pl.selected.insert(i);
            pl.anchor = Some(i);
        }
        ui.ctx().data_mut(|d| d.insert_temp(drag_id, true));
    }
    if dragging {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        // Keeps edge scrolling going while the pointer holds still.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        if ui.input(|i| i.pointer.any_released()) {
            ui.ctx().data_mut(|d| d.remove::<bool>(drag_id));
            if let Some(gap) = drop_gap
                && app.playlists[active].move_selected(gap)
            {
                app.after_playlist_edit();
            }
        }
    }

    let pl = &mut app.playlists[active];
    if let Some((i, m)) = clicked {
        if m.shift
            && let Some(a) = pl.anchor
        {
            let (lo, hi) = (a.min(i), a.max(i));
            if !m.ctrl {
                pl.selected.clear();
            }
            pl.selected.extend(lo..=hi);
        } else if m.ctrl {
            if !pl.selected.remove(&i) {
                pl.selected.insert(i);
            }
            pl.anchor = Some(i);
        } else {
            pl.selected.clear();
            pl.selected.insert(i);
            pl.anchor = Some(i);
        }
    }
    if let Some(i) = play {
        app.play_index(active, i);
    }
    if remove_ctx {
        app.remove_selected();
    }
}

/// Scroll offset that brings `row` into a list view of `(offset, height)`, with
/// the row a third of the way down; `None` when the row is already fully in view.
fn scroll_target(row: usize, (top, height): (f32, f32)) -> Option<f32> {
    let y = row as f32 * ROW_H;
    if y >= top && y + ROW_H <= top + height {
        return None;
    }
    Some((y - height / 3.0).max(0.0))
}

fn footer(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        let mut edited = false;
        ui.menu_button("ADD", |ui| {
            if ui.button("File / folder / M3U…").clicked() {
                app.prompt = Some(Prompt {
                    kind: PromptKind::AddPath,
                    text: String::new(),
                    focus: true,
                });
                ui.close();
            }
            if ui.button("Whole library").clicked() {
                let all: Vec<usize> = (0..app.library.len()).collect();
                app.add_library_tracks(&all, false);
                ui.close();
            }
        });
        let mut remove_sel = false;
        ui.menu_button("REM", |ui| {
            let pl = &mut app.playlists[app.active];
            if ui.button("Remove selected (Del)").clicked() {
                remove_sel = true;
                ui.close();
            }
            if ui.button("Crop (keep selected)").clicked() {
                pl.crop_selected();
                edited = true;
                ui.close();
            }
            if ui.button("Remove missing files").clicked() {
                pl.remove_dead();
                edited = true;
                ui.close();
            }
            if ui.button("Remove duplicates").clicked() {
                pl.remove_duplicates();
                edited = true;
                ui.close();
            }
            if ui.button("Remove duplicate songs (tags)")
                .on_hover_text("Same artist and title, length within 2 s; keeps the first")
                .clicked()
            {
                pl.remove_duplicate_songs();
                edited = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Clear playlist").clicked() {
                pl.clear();
                edited = true;
                ui.close();
            }
        });
        ui.menu_button("SEL", |ui| {
            let pl = &mut app.playlists[app.active];
            if ui.button("Select all (Ctrl+A)").clicked() {
                pl.select_all();
                ui.close();
            }
            if ui.button("Select none").clicked() {
                pl.select_none();
                ui.close();
            }
            if ui.button("Invert selection").clicked() {
                pl.invert_selection();
                ui.close();
            }
        });
        ui.menu_button("MISC", |ui| {
            let pl = &mut app.playlists[app.active];
            let lc = |s: &Option<String>| s.as_deref().unwrap_or("").to_lowercase();
            if ui.button("Sort by title").clicked() {
                pl.sort_by(|a, b| a.display_title().to_lowercase().cmp(&b.display_title().to_lowercase()));
                edited = true;
                ui.close();
            }
            if ui.button("Sort by artist").clicked() {
                pl.sort_by(|a, b| {
                    (lc(&a.artist), lc(&a.album), a.disc_no, a.track_no)
                        .cmp(&(lc(&b.artist), lc(&b.album), b.disc_no, b.track_no))
                });
                edited = true;
                ui.close();
            }
            if ui.button("Sort by album").clicked() {
                pl.sort_by(|a, b| {
                    (lc(&a.album), a.disc_no, a.track_no).cmp(&(lc(&b.album), b.disc_no, b.track_no))
                });
                edited = true;
                ui.close();
            }
            if ui.button("Sort by path").clicked() {
                pl.sort_by(|a, b| a.path.cmp(&b.path));
                edited = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Reverse").clicked() {
                pl.reverse();
                edited = true;
                ui.close();
            }
            if ui.button("Randomize").clicked() {
                pl.randomize();
                edited = true;
                ui.close();
            }
        });
        ui.menu_button("LIST", |ui| {
            let mut kind = None;
            if ui.button("New…").clicked() {
                kind = Some(PromptKind::NewPlaylist);
            }
            if ui.button("Rename…").clicked() {
                kind = Some(PromptKind::RenamePlaylist(app.active));
            }
            if ui.button("Import M3U…").clicked() {
                kind = Some(PromptKind::ImportM3u);
            }
            if ui.button("Export M3U…").clicked() {
                kind = Some(PromptKind::ExportM3u);
            }
            ui.separator();
            if ui.button("Delete playlist").clicked() {
                app.delete_playlist(app.active);
                ui.close();
            }
            if let Some(kind) = kind {
                let music = app
                    .cfg
                    .library_roots
                    .first()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "~".into());
                let text = match &kind {
                    PromptKind::RenamePlaylist(i) => app.playlists[*i].name.clone(),
                    PromptKind::ExportM3u => {
                        format!("{music}/{}.m3u8", app.playlists[app.active].name)
                    }
                    PromptKind::ImportM3u => format!("{music}/"),
                    _ => String::new(),
                };
                app.prompt = Some(Prompt {
                    kind,
                    text,
                    focus: true,
                });
                ui.close();
            }
        });
        if remove_sel {
            app.remove_selected();
        }
        if edited {
            app.after_playlist_edit();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let pl = &app.playlists[app.active];
            let total = std::time::Duration::from_millis(pl.total_duration_ms());
            let sel = if pl.selected.is_empty() {
                String::new()
            } else {
                format!("{} sel / ", pl.selected.len())
            };
            ui.label(
                RichText::new(format!("{sel}{} tracks  {}", pl.len(), format_duration(total)))
                    .monospace()
                    .color(theme.text_dim),
            );
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_target_only_when_out_of_view() {
        // View shows rows 10..20 (170..340 px).
        let view = (170.0, 170.0);
        assert_eq!(scroll_target(10, view), None);
        assert_eq!(scroll_target(19, view), None);
        // Below or above the view: row lands a third of the way down.
        assert_eq!(scroll_target(20, view), Some(20.0 * ROW_H - 170.0 / 3.0));
        assert_eq!(scroll_target(9, view), Some(9.0 * ROW_H - 170.0 / 3.0));
        // Partly hidden counts as out of view.
        assert!(scroll_target(10, (171.0, 170.0)).is_some());
        // Near the top the offset clamps to 0.
        assert_eq!(scroll_target(1, (500.0, 170.0)), Some(0.0));
        // No view stored yet (NaN offset): always scroll.
        assert!(scroll_target(0, (f32::NAN, 170.0)).is_some());
    }
}
