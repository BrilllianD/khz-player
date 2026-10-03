//! Library browser: search, rescan, Artist > Album > Track tree.

use egui::{CollapsingHeader, RichText, Ui};

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
    /// Lowercased search text per track index.
    hay: Vec<String>,
    query: String,
    pub artists: Vec<Artist>,
    pub matches: usize,
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
        let mut order: Vec<usize> = (0..library.len()).collect();
        order.sort_by_cached_key(|&i| {
            let t = &library[i];
            (
                t.group_artist().to_lowercase(),
                t.group_album().to_lowercase(),
                t.disc_no,
                t.track_no,
                &t.path,
            )
        });
        cache.order = order;
        cache.hay = library.iter().map(haystack).collect();
        cache.generation = Some(generation);
    }
    cache.query = query.to_string();
    let terms: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
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

    if let Some(a) = tree(ui, &app.library_cache, &app.library, searching, &theme) {
        act = Some(a);
    }

    match act {
        Some(Act::Add(v)) => app.add_library_tracks(&v, false),
        Some(Act::Replace(v)) => app.add_library_tracks(&v, true),
        None => {}
    }
}

/// Artist > Album > Track tree for the cached view; returns the requested action.
pub fn tree(ui: &mut Ui, cache: &Cache, lib: &[Track], searching: bool, theme: &Theme) -> Option<Act> {
    let mut act: Option<Act> = None;
    egui::ScrollArea::vertical()
        .id_salt("library-tree")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for artist in &cache.artists {
                let header = CollapsingHeader::new(
                    RichText::new(&artist.label).color(theme.text_bright),
                )
                .id_salt(("artist", &artist.name))
                .open(if searching && cache.matches <= 200 { Some(true) } else { None })
                .show(ui, |ui| {
                    for album in &artist.albums {
                        let h = CollapsingHeader::new(
                            RichText::new(&album.label).color(theme.text),
                        )
                        .id_salt(("album", &artist.name, &album.name))
                        .open(if searching && cache.matches <= 60 { Some(true) } else { None })
                        .show(ui, |ui| {
                            for &i in &album.tracks {
                                let t = &lib[i];
                                let no = t.track_no.map(|n| format!("{n:02}. ")).unwrap_or_default();
                                let dur = t.duration().map(format_duration).unwrap_or_default();
                                let r = ui.add(
                                    egui::Button::selectable(
                                        false,
                                        RichText::new(format!("{no}{}  {dur}", t.display_title()))
                                            .monospace()
                                            .size(11.0),
                                    )
                                    .frame_when_inactive(false),
                                );
                                if r.double_clicked() {
                                    act = Some(Act::Replace(vec![i]));
                                }
                                r.context_menu(|ui| {
                                    if ui.button("Add to playlist").clicked() {
                                        act = Some(Act::Add(vec![i]));
                                        ui.close();
                                    }
                                    if ui.button("Play now").clicked() {
                                        act = Some(Act::Replace(vec![i]));
                                        ui.close();
                                    }
                                });
                                r.on_hover_text(t.path.to_string_lossy());
                            }
                        });
                        h.header_response.context_menu(|ui| {
                            if ui.button("Add album to playlist").clicked() {
                                act = Some(Act::Add(album.tracks.clone()));
                                ui.close();
                            }
                            if ui.button("Play album (replace playlist)").clicked() {
                                act = Some(Act::Replace(album.tracks.clone()));
                                ui.close();
                            }
                        });
                    }
                });
                header.header_response.context_menu(|ui| {
                    if ui.button("Add artist to playlist").clicked() {
                        act = Some(Act::Add(artist.tracks()));
                        ui.close();
                    }
                    if ui.button("Play artist (replace playlist)").clicked() {
                        act = Some(Act::Replace(artist.tracks()));
                        ui.close();
                    }
                });
            }
        });
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
}
