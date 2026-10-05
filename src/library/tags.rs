//! Reads tags and audio properties with lofty.

use std::path::Path;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::prelude::*;
use lofty::probe::Probe;

use super::Track;

/// Extensions the scanner picks up. Every one must decode with the symphonia
/// features enabled in `Cargo.toml`; Opus, WavPack, APE and Musepack do not.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "mp2", "mp1", "flac", "ogg", "oga", "m4a", "mp4", "aac", "wav", "aif", "aiff", "aifc",
];

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

pub fn file_stamp(path: &Path) -> (i64, i64) {
    match std::fs::metadata(path) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (mtime, m.len() as i64)
        }
        Err(_) => (0, 0),
    }
}

fn clean(s: Option<std::borrow::Cow<'_, str>>) -> Option<String> {
    s.map(|v| fix_cp1251(v.trim()))
        .filter(|v| !v.is_empty() && !is_lost(v))
}

/// Tags written through a lossy codepage come out as "???????"; nothing can be
/// recovered, so treat them as missing and let the file name show instead.
/// A lone "?" is a real title (XXXTentacion's album), so it takes two.
fn is_lost(s: &str) -> bool {
    s.matches('?').count() >= 2 && !s.chars().any(char::is_alphanumeric)
}

/// Old Russian rips store CP1251 bytes in tags declared as Latin-1, which decode
/// to strings like "ËÅÌÏÉÀÄÀ". Re-decode when the text is only Latin-1 and most
/// of its letters fall in the CP1251 Cyrillic range (0xC0..=0xFF).
pub fn fix_cp1251(s: &str) -> String {
    if s.chars().any(|c| c as u32 > 0xFF) {
        return s.to_string();
    }
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    let high = s.chars().filter(|&c| (0xC0..=0xFF).contains(&(c as u32))).count();
    if high == 0 || high * 2 < letters {
        return s.to_string();
    }
    s.chars()
        .map(|c| match c as u32 {
            b @ 0xC0..=0xFF => char::from_u32(0x0410 + (b - 0xC0)).unwrap_or(c),
            0xA8 => 'Ё',
            0xB8 => 'ё',
            0xB9 => '№',
            0xAB => '«',
            0xBB => '»',
            0x96 => '–',
            0x97 => '—',
            _ => c,
        })
        .collect()
}

/// Reads metadata; on failure returns a track with only path and file stamp,
/// so the title falls back to the file stem.
pub fn read(path: &Path) -> Track {
    let (mtime, size) = file_stamp(path);
    let mut t = Track {
        path: path.to_path_buf(),
        mtime,
        size,
        ..Default::default()
    };
    // Relaxed: skip malformed frames (e.g. a bogus TDRC date) instead of failing the file.
    let opts = ParseOptions::new()
        .parsing_mode(ParsingMode::Relaxed)
        .read_cover_art(false)
        .max_junk_bytes(64 * 1024);
    let tagged = match Probe::open(path).and_then(|p| p.options(opts).read()) {
        Ok(f) => f,
        Err(e) => {
            tracing::debug!("tags {}: {e}", path.display());
            return t;
        }
    };
    let props = tagged.properties();
    let dur = props.duration();
    if !dur.is_zero() {
        t.duration_ms = Some(dur.as_millis() as u64);
    }
    t.bitrate = props.audio_bitrate().or(props.overall_bitrate());
    t.sample_rate = props.sample_rate();
    t.channels = props.channels();
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        t.title = clean(tag.title());
        t.artist = clean(tag.artist());
        t.album = clean(tag.album());
        t.genre = clean(tag.genre());
        t.track_no = tag.track();
        t.disc_no = tag.disk();
        t.year = tag.date().map(|d| d.year as u32).filter(|&y| y > 0);
        t.album_artist = tag
            .get_string(ItemKey::AlbumArtist)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    t
}

#[cfg(test)]
mod tests {
    use super::{AUDIO_EXTENSIONS, fix_cp1251, is_audio, is_lost};
    use std::path::Path;

    #[test]
    fn is_audio_matches_decodable_set() {
        // Containers and codecs covered by the enabled symphonia features.
        const DECODABLE: &[&str] = &[
            "mp3", "mp2", "mp1", "flac", "ogg", "oga", "m4a", "mp4", "aac", "wav", "aif", "aiff",
            "aifc",
        ];
        for ext in AUDIO_EXTENSIONS {
            assert!(DECODABLE.contains(ext), "{ext} listed but not decodable");
        }
        for ext in ["opus", "wv", "ape", "mpc", "txt"] {
            assert!(!is_audio(Path::new(&format!("a.{ext}"))), "{ext}");
        }
        assert!(is_audio(Path::new("a.AIFF")));
    }

    #[test]
    fn aiff_opens_in_decoder() {
        let dir = std::env::temp_dir().join(format!("khz-aiff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.aiff");
        crate::library::scanner::tests::write_aiff(&path, 8000);
        let mut d = crate::audio::decoder::Decoder::open(&path).unwrap();
        assert_eq!(d.info.sample_rate, 8000);
        assert_eq!(d.info.channels, 1);
        let mut out = Vec::new();
        assert!(d.next_frames(&mut out).unwrap());
        assert!(!out.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cp1251_mojibake_is_repaired() {
        assert_eq!(fix_cp1251("Ïðèâåò"), "Привет");
        assert_eq!(fix_cp1251("Àðèÿ - Îáìàí"), "Ария - Обман");
        assert_eq!(fix_cp1251("¨æèê"), "Ёжик");
    }

    #[test]
    fn question_mark_tags_are_lost() {
        assert!(is_lost("???????"));
        assert!(is_lost("??,???"));
        assert!(!is_lost("Am I Evil?"));
        assert!(!is_lost("!!!"));
        assert!(!is_lost("?"));
    }

    #[test]
    fn real_latin1_and_unicode_untouched() {
        assert_eq!(fix_cp1251("Beyoncé"), "Beyoncé");
        assert_eq!(fix_cp1251("Sigur Rós - Hoppípolla"), "Sigur Rós - Hoppípolla");
        assert_eq!(fix_cp1251("Ария"), "Ария");
        assert_eq!(fix_cp1251("Plain ASCII"), "Plain ASCII");
    }
}
