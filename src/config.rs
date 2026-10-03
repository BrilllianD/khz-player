use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::playlist::Repeat;

pub const BANDS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EqConfig {
    pub enabled: bool,
    pub auto: bool,
    pub preamp: f32,
    pub bands: [f32; BANDS],
    pub preset: String,
}

impl Default for EqConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto: false,
            preamp: 0.0,
            bands: [0.0; BANDS],
            preset: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub w: Option<f32>,
    pub h: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub library_roots: Vec<PathBuf>,
    pub font_path: Option<PathBuf>,
    pub volume: f32,
    pub balance: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub last_playlist: Option<String>,
    pub last_track_index: Option<usize>,
    pub show_eq: bool,
    pub show_playlist: bool,
    pub show_library: bool,
    pub time_remaining: bool,
    pub eq: EqConfig,
    pub window: WindowConfig,
}

impl Default for Config {
    fn default() -> Self {
        let music = directories::UserDirs::new()
            .and_then(|u| u.audio_dir().map(|p| p.to_path_buf()))
            .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join("Music")));
        Self {
            version: 1,
            library_roots: music.into_iter().collect(),
            font_path: None,
            volume: 0.8,
            balance: 0.0,
            shuffle: false,
            repeat: Repeat::Off,
            last_playlist: None,
            last_track_index: None,
            show_eq: true,
            show_playlist: true,
            show_library: false,
            time_remaining: false,
            eq: EqConfig::default(),
            window: WindowConfig::default(),
        }
    }
}

pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "rmp")
}

pub fn config_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".rmp"))
}

pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".rmp"))
}

impl Config {
    pub fn path() -> PathBuf {
        config_dir().join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str(&s) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("bad config {}: {e}", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut c = Config::default();
        c.eq.bands[3] = 4.5;
        c.repeat = Repeat::One;
        let s = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.eq.bands[3], 4.5);
        assert_eq!(back.repeat, Repeat::One);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let c: Config = toml::from_str("volume = 0.3\n[eq]\nenabled = true\n").unwrap();
        assert_eq!(c.volume, 0.3);
        assert!(c.eq.enabled);
        assert!(c.show_playlist);
    }
}
