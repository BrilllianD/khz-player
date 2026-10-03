//! Minimal M3U / M3U8 reader and writer (#EXTM3U + #EXTINF).

use std::path::{Component, Path, PathBuf};

use crate::library::Track;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub path: PathBuf,
    pub duration_secs: Option<i64>,
    pub title: Option<String>,
}

/// Parses playlist text. Relative paths are resolved against `base_dir`.
/// URLs are skipped.
pub fn parse(text: &str, base_dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut pending: Option<(Option<i64>, Option<String>)> = None;
    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            let (dur, title) = match info.split_once(',') {
                Some((d, t)) => (d, Some(t.trim().to_string()).filter(|t| !t.is_empty())),
                None => (info, None),
            };
            // Duration may be followed by attributes: `#EXTINF:123 tvg-id="x",Title`.
            let dur = dur
                .split_whitespace()
                .next()
                .and_then(|d| d.parse::<f64>().ok())
                .map(|d| d as i64)
                .filter(|&d| d >= 0);
            pending = Some((dur, title));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if line.contains("://") && !line.starts_with("file://") {
            pending = None;
            continue;
        }
        let raw = line.strip_prefix("file://").unwrap_or(line);
        let raw = percent_decode(raw, line.starts_with("file://"));
        let p = PathBuf::from(raw.replace('\\', "/"));
        let path = if p.is_absolute() {
            normalize(&p)
        } else {
            normalize(&base_dir.join(p))
        };
        let (duration_secs, title) = pending.take().unwrap_or((None, None));
        out.push(Entry {
            path,
            duration_secs,
            title,
        });
    }
    out
}

pub fn read(path: &Path) -> anyhow::Result<Vec<Entry>> {
    let bytes = std::fs::read(path)?;
    // Plain .m3u may be Latin-1; lossy decoding keeps ASCII paths intact.
    let text = String::from_utf8_lossy(&bytes);
    let base = path.parent().unwrap_or(Path::new("/"));
    Ok(parse(&text, base))
}

/// Writes extended M3U. Paths under `base_dir` are written relative to it.
pub fn render(tracks: &[Track], base_dir: Option<&Path>) -> String {
    let mut s = String::from("#EXTM3U\n");
    for t in tracks {
        let secs = t.duration_ms.map(|ms| (ms / 1000) as i64).unwrap_or(-1);
        s.push_str(&format!("#EXTINF:{secs},{}\n", t.display_name()));
        let p = base_dir
            .and_then(|b| t.path.strip_prefix(b).ok())
            .unwrap_or(&t.path);
        s.push_str(&p.to_string_lossy());
        s.push('\n');
    }
    s
}

pub fn write(path: &Path, tracks: &[Track]) -> anyhow::Result<()> {
    std::fs::write(path, render(tracks, path.parent()))?;
    Ok(())
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

fn percent_decode(s: &str, enabled: bool) -> String {
    if !enabled || !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extended() {
        let text = "#EXTM3U\n#EXTINF:225,Nero - Satisfy\nNero - Satisfy.mp3\n\n#EXTINF:-1,\n/abs/b.flac\nhttp://radio/stream\n# comment\nsub/../c.ogg\n";
        let e = parse(text, Path::new("/music"));
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].path, PathBuf::from("/music/Nero - Satisfy.mp3"));
        assert_eq!(e[0].duration_secs, Some(225));
        assert_eq!(e[0].title.as_deref(), Some("Nero - Satisfy"));
        assert_eq!(e[1].path, PathBuf::from("/abs/b.flac"));
        assert_eq!(e[1].duration_secs, None);
        assert_eq!(e[2].path, PathBuf::from("/music/c.ogg"));
        assert_eq!(e[2].title, None);
    }

    #[test]
    fn parse_file_url_and_bom() {
        let e = parse("\u{feff}file:///a/b%20c.mp3\r\n", Path::new("/x"));
        assert_eq!(e[0].path, PathBuf::from("/a/b c.mp3"));
    }

    #[test]
    fn roundtrip() {
        let mut t = Track::from_path(PathBuf::from("/music/Rock/a.mp3"));
        t.artist = Some("A".into());
        t.title = Some("B".into());
        t.duration_ms = Some(61_500);
        let t2 = Track::from_path(PathBuf::from("/other/x.flac"));
        let text = render(&[t, t2], Some(Path::new("/music")));
        assert!(text.contains("#EXTINF:61,A - B\nRock/a.mp3\n"));
        let e = parse(&text, Path::new("/music"));
        assert_eq!(e[0].path, PathBuf::from("/music/Rock/a.mp3"));
        assert_eq!(e[1].path, PathBuf::from("/other/x.flac"));
        assert_eq!(e[1].duration_secs, None);
    }
}
