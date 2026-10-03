//! Reads tags and audio properties with lofty.

use std::path::Path;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::prelude::*;
use lofty::probe::Probe;

use super::Track;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "wv", "ape", "mpc",
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
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
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


