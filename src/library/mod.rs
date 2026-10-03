pub mod db;
pub mod scanner;
pub mod tags;

use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Track {
    pub path: PathBuf,
    pub mtime: i64,
    pub size: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub duration_ms: Option<u64>,
    pub bitrate: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
}

impl Track {
    /// Bare track with only a path; title falls back to the file stem.
    pub fn from_path(path: PathBuf) -> Self {
        Self {
            path,
            ..Default::default()
        }
    }

    pub fn file_stem(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }

    pub fn display_title(&self) -> String {
        match &self.title {
            Some(t) if !t.trim().is_empty() => t.clone(),
            _ => self.file_stem(),
        }
    }

    /// "Artist - Title", or just the title when the artist is unknown.
    pub fn display_name(&self) -> String {
        match &self.artist {
            Some(a) if !a.trim().is_empty() => format!("{a} - {}", self.display_title()),
            _ => self.display_title(),
        }
    }

    pub fn duration(&self) -> Option<Duration> {
        self.duration_ms.map(Duration::from_millis)
    }

    /// Artist used for library grouping: album artist first, then track artist.
    pub fn group_artist(&self) -> &str {
        self.album_artist
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .or(self.artist.as_deref().filter(|s| !s.trim().is_empty()))
            .unwrap_or("Unknown Artist")
    }

    pub fn group_album(&self) -> &str {
        self.album
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Unknown Album")
    }
}

pub fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
