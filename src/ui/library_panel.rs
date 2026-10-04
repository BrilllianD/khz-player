//! Library browser: search, rescan, Artist > Album > Track tree.

use std::collections::HashSet;

use egui::{Align2, FontId, Rect, RichText, Sense, Ui, pos2, vec2};

use crate::app::App;
use crate::library::{Track, format_duration};
use crate::theme::Theme;

pub struct Album {
    pub name: String,
    /// Header text, "Name [year]".
    pub label: String,
    pub tracks: Vec<usize>,
}

pub struct Artist {
    pub name: String,
    /// Header text, "Name  (count)".
    pub label: String,
    pub albums: Vec<Album>,
    pub count: usize,
}

impl Artist {
    pub fn tracks(&self) -> Vec<usize> {
        self.albums.iter().flat_map(|a| a.tracks.iter().copied()).collect()
    }
}

#[derive(Default)]
pub struct Cache {
    /// Library generation `order` and `hay` were built for.
    generation: Option<u64>,
    /// Every track index in tree order (artist, album, disc, track, path).
    order: Vec<usize>,
    /// Lowercased search text per track index; built on the first search.
    hay: Vec<String>,
    query: String,
    pub artists: Vec<Artist>,
    pub matches: usize,
    /// Expanded artist headers, by artist name.
    open_artists: HashSet<String>,
    /// Expanded album headers, by (artist, album) name.
    open_albums: HashSet<(String, String)>,
}

fn haystack(t: &Track) -> String {
    format!(
        "{} {} {} {} {}",
        t.artist.as_deref().unwrap_or(""),
        t.album_artist.as_deref().unwrap_or(""),
        t.album.as_deref().unwrap_or(""),
        t.display_title(),
        t.genre.as_deref().unwrap_or("")
    )
    .to_lowercase()
}

/// Rebuilds the grouped view when the library or the query changes. Sorting and
/// search text depend only on the library, so a new query just filters.
pub fn refresh(cache: &mut Cache, library: &[Track], generation: u64, query: &str) {
    let fresh_lib = cache.generation != Some(generation);
    if !fresh_lib && cache.query == query {
        return;
    }
    if fresh_lib {
        let names: Vec<(String, String)> = library
            .iter()
            .map(|t| (t.group_artist().to_lowercase(), t.group_album().to_lowercase()))
            .collect();
        let key = |i: usize| {
            let t = &library[i];
            (&names[i].0, &names[i].1, t.disc_no, t.track_no, &t.path)
        };
        let mut order: Vec<usize> = (0..library.len()).collect();
        order.sort_by(|&a, &b| key(a).cmp(&key(b)));
        cache.order = order;
        cache.hay.clear();
        cache.generation = Some(generation);
    }
    cache.query = query.to_string();
    let terms: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if !terms.is_empty() && cache.hay.is_empty() {
        cache.hay = library.iter().map(haystack).collect();
    }
    let mut artists: Vec<Artist> = Vec::new();
    let mut matches = 0;
    for i in cache.order.iter().copied() {
        if !terms.iter().all(|w| cache.hay[i].contains(w.as_str())) {
            continue;
        }
        matches += 1;
        let t = &library[i];
        let (ar, al) = (t.group_artist(), t.group_album());
        if artists.last().is_none_or(|a| !a.name.eq_ignore_ascii_case(ar)) {
            artists.push(Artist {
                name: ar.to_string(),
                label: String::new(),
                albums: Vec::new(),
                count: 0,
            });
        }
        let artist = artists.last_mut().expect("artist");
        artist.count += 1;
        if artist
            .albums
            .last()
            .is_none_or(|a| !a.name.eq_ignore_ascii_case(al))
        {
            let year = t.year.map(|y| format!(" [{y}]")).unwrap_or_default();
            artist.albums.push(Album {
                name: al.to_string(),
                label: format!("{al}{year}"),
                tracks: Vec::new(),
            });
        }
        artist.albums.last_mut().expect("album").tracks.push(i);
    }
    for a in &mut artists {
        a.label = format!("{}  ({})", a.name, a.count);
    }
    cache.matches = matches;
    cache.artists = artists;
}

pub enum Act {
    Add(Vec<usize>),
    Replace(Vec<usize>),
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let theme = app.theme.clone();
    ui.horizontal(|ui| {
        ui.label(RichText::new("LIBRARY").monospace().color(theme.text_bright));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let scanning = app.scan_progress.is_some();
            if ui
                .add_enabled(!scanning, egui::Button::new("Rescan"))
                .on_hover_text(
                    app.cfg
                        .library_roots
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
                .clicked()
            {
                app.rescan();
            }
            ui.label(
                RichText::new(format!("{} tracks", app.library.len()))
                    .monospace()
                    .color(theme.text_dim),
            );
        });
    });
    if let Some((done, total)) = app.scan_progress {
        let f = if total == 0 { 0.0 } else { done as f32 / total as f32 };
        ui.add(
            egui::ProgressBar::new(f)
                .text(format!("Scanning {done}/{total}"))
                .desired_height(14.0),
        );
    }
    ui.add(
        egui::TextEdit::singleline(&mut app.library_search)
            .hint_text("Search artist, album, title…")
            .desired_width(f32::INFINITY),
    );

    refresh(
        &mut app.library_cache,
        &app.library,
        app.library_gen,
        &app.library_search,
    );
    let searching = !app.library_search.trim().is_empty();
    let mut act: Option<Act> = None;

    ui.horizontal(|ui| {
        let n = app.library_cache.matches;
        ui.label(RichText::new(format!("{n} shown")).small().color(theme.text_dim));
        if searching && n > 0 && ui.small_button("Add all shown").clicked() {
            act = Some(Act::Add(
                app.library_cache
                    .artists
                    .iter()
                    .flat_map(|a| a.albums.iter().flat_map(|al| al.tracks.iter().copied()))
                    .collect(),
            ));
        }
    });
    ui.separator();

    if let Some(a) = tree(ui, &mut app.library_cache, &app.library, searching, &theme) {
        act = Some(a);
    }

    match act {
        Some(Act::Add(v)) => app.add_library_tracks(&v, false),
        Some(Act::Replace(v)) => app.add_library_tracks(&v, true),
        None => {}
    }
}

/// One line of the tree as drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Index into `Cache::artists`.
    Artist(usize),
    /// Artist index, album index within it.
    Album(usize, usize),
    /// Library track index.
    Track(usize),
}

const ROW_H: f32 = 18.0;
const INDENT: f32 = 14.0;

impl Cache {
    fn artist_open(&self, a: &Artist, searching: bool) -> bool {
        (searching && self.matches <= 200) || self.open_artists.contains(&a.name)
    }

    fn album_open(&self, a: &Artist, al: &Album, searching: bool) -> bool {
        (searching && self.matches <= 60)
            || self.open_albums.contains(&(a.name.clone(), al.name.clone()))
    }

    /// Lines under the open headers, in tree order.
    pub fn visible_rows(&self, searching: bool) -> Vec<Row> {
        let mut rows = Vec::new();
        for (ai, a) in self.artists.iter().enumerate() {
            rows.push(Row::Artist(ai));
            if !self.artist_open(a, searching) {
                continue;
            }
            for (bi, al) in a.albums.iter().enumerate() {
                rows.push(Row::Album(ai, bi));
                if self.album_open(a, al, searching) {
                    rows.extend(al.tracks.iter().map(|&i| Row::Track(i)));
                }
            }
        }
        rows
    }
}

/// Small open/closed triangle for a header row.
fn arrow(p: &egui::Painter, at: egui::Pos2, open: bool, color: egui::Color32) {
    let s = 3.5;
    let pts = if open {
        vec![at + vec2(-s, -s * 0.6), at + vec2(s, -s * 0.6), at + vec2(0.0, s * 0.8)]
    } else {
        vec![at + vec2(-s * 0.6, -s), at + vec2(s * 0.8, 0.0), at + vec2(-s * 0.6, s)]
    };
    p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// Artist > Album > Track tree for the cached view; returns the requested action.
/// Only the rows in view are laid out, so a big library costs nothing per frame.
pub fn tree(ui: &mut Ui, cache: &mut Cache, lib: &[Track], searching: bool, theme: &Theme) -> Option<Act> {
    let mut act: Option<Act> = None;
    let mut toggle: Option<Row> = None;
    let rows = cache.visible_rows(searching);
    let header_font = egui::TextStyle::Body.resolve(ui.style());
    let track_font = FontId::monospace(11.0);
    egui::ScrollArea::vertical()
        .id_salt("library-tree")
        .auto_shrink([false, false])
        .show_rows(ui, ROW_H, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for &row in &rows[range] {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
                let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                if resp.hovered() {
                    p.rect_filled(rect, 0.0, theme.bg_dark);
                }
                let y = rect.center().y;
                match row {
                    Row::Artist(ai) => {
                        let a = &cache.artists[ai];
                        let x = rect.left() + 6.0;
                        arrow(&p, pos2(x, y), cache.artist_open(a, searching), theme.text_dim);
                        p.text(
                            pos2(x + 10.0, y),
                            Align2::LEFT_CENTER,
                            &a.label,
                            header_font.clone(),
                            theme.text_bright,
                        );
                        if resp.clicked() {
                            toggle = Some(row);
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Add artist to playlist").clicked() {
                                act = Some(Act::Add(a.tracks()));
                                ui.close();
                            }
                            if ui.button("Play artist (replace playlist)").clicked() {
                                act = Some(Act::Replace(a.tracks()));
                                ui.close();
                            }
                        });
                    }
                    Row::Album(ai, bi) => {
                        let a = &cache.artists[ai];
                        let al = &a.albums[bi];
                        let x = rect.left() + 6.0 + INDENT;
                        arrow(&p, pos2(x, y), cache.album_open(a, al, searching), theme.text_dim);
                        p.text(
                            pos2(x + 10.0, y),
                            Align2::LEFT_CENTER,
                            &al.label,
                            header_font.clone(),
                            theme.text,
                        );
                        if resp.clicked() {
                            toggle = Some(row);
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Add album to playlist").clicked() {
                                act = Some(Act::Add(al.tracks.clone()));
                                ui.close();
                            }
                            if ui.button("Play album (replace playlist)").clicked() {
                                act = Some(Act::Replace(al.tracks.clone()));
                                ui.close();
                            }
                        });
                    }
                    Row::Track(i) => {
                        let t = &lib[i];
                        let dur = t.duration().map(format_duration).unwrap_or_default();
                        let dur_rect = p.text(
                            pos2(rect.right() - 6.0, y),
                            Align2::RIGHT_CENTER,
                            &dur,
                            track_font.clone(),
                            theme.text_dim,
                        );
                        let no = t.track_no.map(|n| format!("{n:02}. ")).unwrap_or_default();
                        let name_clip = Rect::from_min_max(
                            pos2(rect.left(), rect.top()),
                            pos2(dur_rect.left() - 8.0, rect.bottom()),
                        );
                        p.with_clip_rect(name_clip.intersect(p.clip_rect())).text(
                            pos2(rect.left() + 6.0 + 2.0 * INDENT, y),
                            Align2::LEFT_CENTER,
                            format!("{no}{}", t.display_title()),
                            track_font.clone(),
                            theme.text,
                        );
                        if resp.double_clicked() {
                            act = Some(Act::Replace(vec![i]));
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Add to playlist").clicked() {
                                act = Some(Act::Add(vec![i]));
                                ui.close();
                            }
                            if ui.button("Play now").clicked() {
                                act = Some(Act::Replace(vec![i]));
                                ui.close();
                            }
                        });
                        resp.on_hover_text(t.path.to_string_lossy());
                    }
                }
            }
        });
    match toggle {
        Some(Row::Artist(ai)) => {
            let name = cache.artists[ai].name.clone();
            if !cache.open_artists.remove(&name) {
                cache.open_artists.insert(name);
            }
        }
        Some(Row::Album(ai, bi)) => {
            let a = &cache.artists[ai];
            let key = (a.name.clone(), a.albums[bi].name.clone());
            if !cache.open_albums.remove(&key) {
                cache.open_albums.insert(key);
            }
        }
        _ => {}
    }
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(artist: &str, album: &str, no: u32, title: &str) -> Track {
        Track {
            path: format!("/m/{artist}/{album}/{no}.mp3").into(),
            artist: Some(artist.into()),
            album: Some(album.into()),
            track_no: Some(no),
            title: Some(title.into()),
            year: Some(2000),
            ..Default::default()
        }
    }

    #[test]
    fn groups_sorts_and_filters() {
        let lib = vec![
            track("beta", "B1", 2, "Two"),
            track("Alpha", "A1", 1, "One"),
            track("beta", "B1", 1, "First"),
            track("alpha", "A2", 1, "Other"),
        ];
        let mut c = Cache::default();
        refresh(&mut c, &lib, 1, "");
        let names: Vec<&str> = c.artists.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(names, ["Alpha  (2)", "beta  (2)"]);
        assert_eq!(c.artists[0].albums[0].label, "A1 [2000]");
        assert_eq!(c.artists[1].tracks(), [2, 0]);
        assert_eq!(c.matches, 4);

        // New query, same library: filters the cached order.
        refresh(&mut c, &lib, 1, "BETA  fir");
        assert_eq!(c.matches, 1);
        assert_eq!(c.artists[0].tracks(), [2]);

        // New library generation rebuilds the order.
        let mut lib2 = lib.clone();
        lib2.push(track("aardvark", "Z", 1, "Zed"));
        refresh(&mut c, &lib2, 2, "");
        assert_eq!(c.artists[0].name, "aardvark");
        assert_eq!(c.matches, 5);
    }

    #[test]
    fn visible_rows_follow_open_headers() {
        let lib = vec![
            track("a", "A1", 1, "One"),
            track("a", "A1", 2, "Two"),
            track("a", "A2", 1, "Other"),
            track("b", "B1", 1, "Bee"),
        ];
        let mut c = Cache::default();
        refresh(&mut c, &lib, 1, "");
        assert_eq!(c.visible_rows(false), [Row::Artist(0), Row::Artist(1)]);

        c.open_artists.insert("a".into());
        c.open_albums.insert(("a".into(), "A1".into()));
        assert_eq!(
            c.visible_rows(false),
            [
                Row::Artist(0),
                Row::Album(0, 0),
                Row::Track(0),
                Row::Track(1),
                Row::Album(0, 1),
                Row::Artist(1),
            ]
        );

        // A small search result opens everything regardless of the sets.
        refresh(&mut c, &lib, 1, "bee");
        assert_eq!(c.visible_rows(true), [Row::Artist(0), Row::Album(0, 0), Row::Track(3)]);
    }
}
