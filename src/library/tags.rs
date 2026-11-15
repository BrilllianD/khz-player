//! Reads tags and audio properties with lofty.

use std::path::Path;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType};

use super::{ReplayGain, Track};

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

fn clean(s: Option<std::borrow::Cow<'_, str>>, legacy: bool) -> Option<String> {
    s.map(|v| if legacy { fix_cp1251(v.trim()) } else { v.trim().to_string() })
        .filter(|v| !v.is_empty() && !is_lost(v))
}

/// Tags written through a lossy codepage come out as "???????"; nothing can be
/// recovered, so treat them as missing and let the file name show instead.
/// A lone "?" is a real title (XXXTentacion's album), so it takes two.
fn is_lost(s: &str) -> bool {
    s.matches('?').count() >= 2 && !s.chars().any(char::is_alphanumeric)
}

/// Old Russian rips store CP1251 bytes in tags declared as Latin-1, which decode
/// to strings like "ËÅÌÏÉÀÄÀ". Re-decode when the text is only Latin-1, at least 3
/// characters fall in the CP1251 Cyrillic range (0xC0..=0xFF) and they make up at
/// least half of the letters. Short real Latin-1 like "Café" stays.
pub fn fix_cp1251(s: &str) -> String {
    if s.chars().any(|c| c as u32 > 0xFF) {
        return s.to_string();
    }
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    let high = s.chars().filter(|&c| (0xC0..=0xFF).contains(&(c as u32))).count();
    if high < 3 || high * 2 < letters {
        return s.to_string();
    }
    s.chars().map(|c| cp1251_char(c as u8)).collect()
}

/// Decodes raw CP1251 bytes (ASCII passes through).
pub fn decode_cp1251(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| cp1251_char(b)).collect()
}

fn cp1251_char(b: u8) -> char {
    match b {
        0xC0..=0xFF => char::from_u32(0x0410 + (b as u32 - 0xC0)).unwrap_or(char::from(b)),
        0xA8 => 'Ё',
        0xB8 => 'ё',
        0xB9 => '№',
        0xAB => '«',
        0xBB => '»',
        0x96 => '–',
        0x97 => '—',
        _ => char::from(b),
    }
}

/// Parses a ReplayGain value such as `-6.54 dB`, `+3.20 dB` or `0.988553`.
fn parse_rg(s: &str) -> Option<f32> {
    let s = s.trim();
    let n = s.len();
    let s = if n >= 2 && s.is_char_boundary(n - 2) && s[n - 2..].eq_ignore_ascii_case("db") {
        &s[..n - 2]
    } else {
        s
    };
    s.trim().replace(',', ".").parse::<f32>().ok().filter(|v| v.is_finite())
}

fn replay_gain(tag: &Tag) -> ReplayGain {
    let get = |k: ItemKey| tag.get_string(k).and_then(parse_rg);
    ReplayGain {
        track_gain_db: get(ItemKey::ReplayGainTrackGain),
        track_peak: get(ItemKey::ReplayGainTrackPeak).filter(|p| *p >= 0.0),
        album_gain_db: get(ItemKey::ReplayGainAlbumGain),
        album_peak: get(ItemKey::ReplayGainAlbumPeak).filter(|p| *p >= 0.0),
    }
}

fn apply_tag(t: &mut Track, tag: &Tag) {
    let legacy = matches!(tag.tag_type(), TagType::Id3v1 | TagType::Id3v2);
    t.title = clean(tag.title(), legacy);
    t.artist = clean(tag.artist(), legacy);
    t.album = clean(tag.album(), legacy);
    t.genre = clean(tag.genre(), legacy);
    t.track_no = tag.track();
    t.disc_no = tag.disk();
    t.year = tag.date().map(|d| d.year as u32).filter(|&y| y > 0);
    t.album_artist = clean(tag.get_string(ItemKey::AlbumArtist).map(std::borrow::Cow::Borrowed), legacy);
    t.replay_gain = replay_gain(tag);
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
        apply_tag(&mut t, tag);
    }
    // mp3gain writes APEv2 next to ID3v2: take the gain from any tag that carries it.
    if t.replay_gain == ReplayGain::default() {
        t.replay_gain = pick_replay_gain(tagged.primary_tag(), tagged.tags());
    }
    t
}

/// ReplayGain of the primary tag if it has any, else of the first other tag that does.
fn pick_replay_gain(primary: Option<&Tag>, all: &[Tag]) -> ReplayGain {
    primary
        .into_iter()
        .chain(all)
        .map(replay_gain)
        .find(|rg| *rg != ReplayGain::default())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        AUDIO_EXTENSIONS, apply_tag, decode_cp1251, fix_cp1251, is_audio, is_lost, parse_rg,
        pick_replay_gain,
    };
    use crate::library::{ReplayGain, Track};
    use lofty::prelude::*;
    use lofty::tag::{Tag, TagType};
    use std::path::Path;

    fn album_artist_of(raw: &str) -> Option<String> {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.insert_text(ItemKey::AlbumArtist, raw.into());
        tag.set_title("Àðèÿ".into());
        let mut t = Track::default();
        apply_tag(&mut t, &tag);
        t.album_artist
    }

    #[test]
    fn parse_rg_handles_db_suffix_sign_comma_and_junk() {
        assert_eq!(parse_rg("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_rg("+3.20 dB"), Some(3.2));
        assert_eq!(parse_rg(" 0.988553 "), Some(0.988553));
        assert_eq!(parse_rg("-6,54 dB"), Some(-6.54));
        assert_eq!(parse_rg("abc"), None);
        assert_eq!(parse_rg(""), None);
        assert_eq!(parse_rg("inf"), None);
    }

    #[test]
    fn replay_gain_keys_are_read() {
        for tt in [TagType::VorbisComments, TagType::Id3v2] {
            let mut tag = Tag::new(tt);
            tag.insert_text(ItemKey::ReplayGainTrackGain, "-6.54 dB".into());
            tag.insert_text(ItemKey::ReplayGainTrackPeak, "0.988553".into());
            tag.insert_text(ItemKey::ReplayGainAlbumGain, "-7.10 dB".into());
            tag.insert_text(ItemKey::ReplayGainAlbumPeak, "1.0".into());
            let mut t = Track::default();
            apply_tag(&mut t, &tag);
            assert_eq!(
                t.replay_gain,
                ReplayGain {
                    track_gain_db: Some(-6.54),
                    track_peak: Some(0.988553),
                    album_gain_db: Some(-7.10),
                    album_peak: Some(1.0),
                }
            );
        }
    }

    #[test]
    fn replay_gain_falls_back_to_secondary_tag_and_primary_wins() {
        let mut id3 = Tag::new(TagType::Id3v2);
        id3.set_title("t".into());
        let mut ape = Tag::new(TagType::Ape);
        ape.insert_text(ItemKey::ReplayGainTrackGain, "-5.00 dB".into());
        let tags = [id3.clone(), ape.clone()];
        let rg = pick_replay_gain(Some(&tags[0]), &tags);
        assert_eq!(rg.track_gain_db, Some(-5.0));

        id3.insert_text(ItemKey::ReplayGainTrackGain, "-9.00 dB".into());
        let tags = [ape, id3];
        let rg = pick_replay_gain(Some(&tags[1]), &tags);
        assert_eq!(rg.track_gain_db, Some(-9.0));

        assert_eq!(pick_replay_gain(None, &[]), ReplayGain::default());
    }

    #[test]
    fn bad_replay_gain_is_none() {
        let mut tag = Tag::new(TagType::VorbisComments);
        tag.insert_text(ItemKey::ReplayGainTrackGain, "loud".into());
        tag.insert_text(ItemKey::ReplayGainTrackPeak, "-0.5".into());
        let mut t = Track::default();
        apply_tag(&mut t, &tag);
        assert_eq!(t.replay_gain, ReplayGain::default());
        let mut t = Track::default();
        apply_tag(&mut t, &Tag::new(TagType::VorbisComments));
        assert_eq!(t.replay_gain, ReplayGain::default());
    }

    #[test]
    fn album_artist_is_cleaned() {
        assert_eq!(album_artist_of("  Ïðèâåò  ").as_deref(), Some("Привет"));
        assert_eq!(album_artist_of("???????"), None);
        assert_eq!(album_artist_of("   "), None);
    }

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

    #[test]
    fn short_latin1_is_not_repaired() {
        assert_eq!(fix_cp1251("Éé"), "Éé");
        assert_eq!(fix_cp1251("Ça"), "Ça");
        assert_eq!(fix_cp1251("Café"), "Café");
    }

    #[test]
    fn decodes_cp1251_bytes() {
        assert_eq!(decode_cp1251(b"\xcf\xf0\xe8\xe2\xe5\xf2 1.mp3"), "Привет 1.mp3");
    }

    #[test]
    fn only_id3_tags_are_repaired() {
        for (ty, want) in [(TagType::VorbisComments, "Ïðèâåò"), (TagType::Id3v2, "Привет")] {
            let mut tag = Tag::new(ty);
            tag.set_title("Ïðèâåò".into());
            let mut t = Track::default();
            apply_tag(&mut t, &tag);
            assert_eq!(t.title.as_deref(), Some(want));
        }
    }
}
