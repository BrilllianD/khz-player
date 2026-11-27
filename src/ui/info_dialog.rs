//! "File info" (Alt+3): read-only modal with the tags, format, ReplayGain and cover of one track.

use std::path::PathBuf;

use egui::{Id, Modal, RichText};

use crate::app::App;
use crate::library::{format_duration, Track};

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(t) = &app.info else {
        return;
    };
    let path = t.path.clone();
    let src_id = Id::new("info-source");
    let cached = ctx.data(|d| d.get_temp::<(PathBuf, Option<String>)>(src_id));
    let source = match cached {
        Some((p, s)) if p == path => s,
        _ => {
            let s = crate::library::tags::source_url(&path);
            ctx.data_mut(|d| d.insert_temp(src_id, (path.clone(), s.clone())));
            s
        }
    };
    let rows = rows(t, source.as_deref());
    app.info_cover.sync_track(ctx, Some(&path));
    let theme = app.theme.clone();
    let tex = app.info_cover.texture().map(|h| (h.id(), h.size_vec2()));

    let mut close = false;
    let resp = Modal::new(Id::new("info")).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new("File info").strong());
        if let Some((id, s)) = tex {
            let k = (160.0 / s.x).min(160.0 / s.y);
            ui.image((id, s * k));
        }
        egui::Grid::new("info-grid").num_columns(2).show(ui, |ui| {
            for (k, v) in &rows {
                ui.label(RichText::new(*k).color(theme.text_dim));
                ui.add(egui::Label::new(v.as_str()).wrap());
                ui.end_row();
            }
        });
        if ui.button("Close").clicked() {
            close = true;
        }
    });
    if close || resp.should_close() {
        app.info = None;
        ctx.data_mut(|d| d.remove::<(PathBuf, Option<String>)>(src_id));
        app.info_cover.sync_track(ctx, None);
    }
}

fn db(v: Option<f32>) -> Option<String> {
    v.map(|v| format!("{v:+.2} dB"))
}

/// Label and value of every known field of `t`, in display order.
pub(crate) fn rows(t: &Track, source: Option<&str>) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = vec![("Path", t.path.display().to_string())];
    let mut push = |k: &'static str, v: Option<String>| {
        if let Some(v) = v.filter(|v| !v.trim().is_empty()) {
            out.push((k, v));
        }
    };
    let text = |s: &Option<String>| s.clone();
    let num = |n: Option<u32>| n.map(|n| n.to_string());
    push("Source", source.map(String::from));
    push("Title", text(&t.title));
    push("Artist", text(&t.artist));
    push("Album", text(&t.album));
    push("Album artist", text(&t.album_artist));
    push("Track", num(t.track_no));
    push("Disc", num(t.disc_no));
    push("Year", num(t.year));
    push("Genre", text(&t.genre));
    push("Duration", t.duration().map(format_duration));
    push(
        "Format",
        t.path.extension().map(|e| e.to_string_lossy().to_uppercase()),
    );
    push("Bitrate", t.bitrate.map(|n| format!("{n} kbps")));
    push("Sample rate", t.sample_rate.map(|n| format!("{n} Hz")));
    push(
        "Channels",
        t.channels.map(|n| match n {
            1 => "mono".to_string(),
            2 => "stereo".to_string(),
            n => n.to_string(),
        }),
    );
    push(
        "Size",
        (t.size > 0).then(|| format!("{:.1} MiB", t.size as f64 / 1_048_576.0)),
    );
    let g = &t.replay_gain;
    if g.track_gain_db.is_none()
        && g.track_peak.is_none()
        && g.album_gain_db.is_none()
        && g.album_peak.is_none()
    {
        push("ReplayGain", Some("none".to_string()));
    } else {
        push("Track gain", db(g.track_gain_db));
        push("Track peak", g.track_peak.map(|v| format!("{v:.6}")));
        push("Album gain", db(g.album_gain_db));
        push("Album peak", g.album_peak.map(|v| format!("{v:.6}")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::ReplayGain;

    fn get<'a>(r: &'a [(&'static str, String)], k: &str) -> Option<&'a str> {
        r.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str())
    }

    #[test]
    fn bare_track() {
        let r = rows(&Track::from_path("/m/a.flac".into()), None);
        let want: Vec<(&str, String)> = vec![
            ("Path", "/m/a.flac".into()),
            ("Format", "FLAC".into()),
            ("ReplayGain", "none".into()),
        ];
        assert_eq!(r, want);
    }

    #[test]
    fn full_track() {
        let t = Track {
            path: "/m/a.flac".into(),
            size: 3 * 1_048_576,
            title: Some("T".into()),
            artist: Some("A".into()),
            duration_ms: Some(61_000),
            bitrate: Some(320),
            sample_rate: Some(44100),
            channels: Some(2),
            replay_gain: ReplayGain {
                track_gain_db: Some(-6.54),
                track_peak: Some(0.988),
                album_gain_db: None,
                album_peak: None,
            },
            ..Default::default()
        };
        let r = rows(&t, None);
        assert_eq!(get(&r, "Channels"), Some("stereo"));
        assert_eq!(get(&r, "Bitrate"), Some("320 kbps"));
        assert_eq!(get(&r, "Sample rate"), Some("44100 Hz"));
        assert_eq!(get(&r, "Size"), Some("3.0 MiB"));
        assert_eq!(get(&r, "Track gain"), Some("-6.54 dB"));
        assert_eq!(get(&r, "Track peak"), Some("0.988000"));
        assert_eq!(get(&r, "Duration"), Some("1:01"));
        assert_eq!(get(&r, "ReplayGain"), None);
        assert_eq!(get(&r, "Album gain"), None);
    }

    #[test]
    fn blank_title_omitted() {
        let t = Track {
            title: Some("  ".into()),
            ..Track::from_path("/m/a.mp3".into())
        };
        assert_eq!(get(&rows(&t, None), "Title"), None);
    }

    #[test]
    fn source_row_after_path() {
        let r = rows(&Track::from_path("/m/a.mp3".into()), Some("https://x/y"));
        assert_eq!(r[1], ("Source", "https://x/y".to_string()));
    }

    #[test]
    fn blank_source_omitted() {
        let r = rows(&Track::from_path("/m/a.mp3".into()), Some("  "));
        assert_eq!(get(&r, "Source"), None);
    }
}
