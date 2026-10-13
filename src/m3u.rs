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
        // A `#` line right after `#EXTINF` is a file name, not a comment (`#EXT*` stays a tag).
        if line.starts_with('#') && (pending.is_none() || line.starts_with("#EXT")) {
            continue;
        }
        let is_file_url = line.starts_with("file://");
        if line.contains("://") && !is_file_url {
            pending = None;
            continue;
        }
        let mut raw = line;
        if is_file_url {
            raw = &line["file://".len()..];
            if let Some(rest) = raw.strip_prefix("localhost")
                && rest.starts_with('/')
            {
                raw = rest;
            } else if !raw.starts_with('/') {
                // Other hosts are remote, like URLs.
                pending = None;
                continue;
            }
        }
        let raw = percent_decode(raw, is_file_url);
        let p = if has_drive(&raw) {
            // A drive-letter path cannot resolve on Linux; assume the file sits next to the list.
            let name = raw[2..].rsplit(['\\', '/']).next().unwrap_or("");
            base_dir.join(name)
        } else if raw.contains('\\') && !raw.contains('/') {
            PathBuf::from(raw.replace('\\', "/"))
        } else {
            PathBuf::from(raw)
        };
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
    // Plain .m3u from Windows tools is usually CP1251 when it is not UTF-8.
    let text = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => crate::library::tags::decode_cp1251(e.as_bytes()),
    };
    let base = path.parent().unwrap_or(Path::new("/"));
    Ok(parse(&text, base))
}

/// Writes extended M3U. Paths under `base_dir` are written relative to it.
pub fn render(tracks: &[Track], base_dir: Option<&Path>) -> String {
    let mut s = String::from("#EXTM3U\n");
    for t in tracks {
        let secs = t.duration_ms.map(|ms| (ms / 1000) as i64).unwrap_or(-1);
        let name = t.display_name().replace(['\r', '\n'], " ");
        s.push_str(&format!("#EXTINF:{secs},{name}\n"));
        let p = base_dir
            .and_then(|b| t.path.strip_prefix(b).ok())
            .unwrap_or(&t.path);
        if p.is_relative() && p.to_string_lossy().starts_with('#') {
            s.push_str("./");
        }
        s.push_str(&p.to_string_lossy());
        s.push('\n');
    }
    s
}

pub fn write(path: &Path, tracks: &[Track]) -> anyhow::Result<()> {
    std::fs::write(path, render(tracks, path.parent()))?;
    Ok(())
}

fn has_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
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

    fn one(text: &str) -> Vec<Entry> {
        parse(text, Path::new("/music"))
    }

    #[test]
    fn hash_name_after_extinf() {
        let e = one("#EXTINF:1,T\n#hash.mp3\n# comment\nb.mp3\n");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].path, PathBuf::from("/music/#hash.mp3"));
        assert_eq!(e[1].path, PathBuf::from("/music/b.mp3"));
        assert!(one("# comment\n").is_empty());
    }

    #[test]
    fn file_localhost() {
        let e = one("file://localhost/a/b%20c.mp3\n");
        assert_eq!(e[0].path, PathBuf::from("/a/b c.mp3"));
    }

    #[test]
    fn file_other_host_skipped() {
        assert!(one("file://host/x.mp3\n").is_empty());
    }

    #[test]
    fn windows_drive_path() {
        let e = one("C:\\Music\\Rock\\a.mp3\n");
        assert_eq!(e[0].path, PathBuf::from("/music/a.mp3"));
    }

    #[test]
    fn windows_relative_path() {
        let e = one("Rock\\b.mp3\n");
        assert_eq!(e[0].path, PathBuf::from("/music/Rock/b.mp3"));
    }

    #[test]
    fn backslash_with_slash_kept() {
        let e = one("/a/x\\y.mp3\n");
        assert_eq!(e[0].path, PathBuf::from("/a/x\\y.mp3"));
    }

    #[test]
    fn extinf_attributes() {
        let e = one("#EXTINF:123 tvg-id=\"x\" group-title=\"y\",Title\na.mp3\n");
        assert_eq!(e[0].duration_secs, Some(123));
        assert_eq!(e[0].title.as_deref(), Some("Title"));
    }

    #[test]
    fn read_cp1251() {
        let dir = crate::config::test_dir("m3u-cp1251");
        let f = dir.join("l.m3u");
        std::fs::write(&f, b"#EXTM3U\n\xcf\xf0\xe8\xe2\xe5\xf2.mp3\n").unwrap();
        let e = read(&f).unwrap();
        assert_eq!(e[0].path, dir.join("Привет.mp3"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn render_newline_and_hash_name() {
        let mut t = Track::from_path(PathBuf::from("/music/a.mp3"));
        t.title = Some("a\nb".into());
        let t2 = Track::from_path(PathBuf::from("/music/#x.mp3"));
        let text = render(&[t, t2], Some(Path::new("/music")));
        assert!(text.contains("#EXTINF:-1,a b\n"));
        assert!(text.contains("\n./#x.mp3\n"));
        let e = parse(&text, Path::new("/music"));
        assert_eq!(e[1].path, PathBuf::from("/music/#x.mp3"));
    }
}
