use std::path::{Path, PathBuf};

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
    /// Position in the last track when khz-player quit while playing or paused.
    pub last_position_ms: Option<u64>,
    pub show_eq: bool,
    pub show_playlist: bool,
    pub show_library: bool,
    pub time_remaining: bool,
    /// Repaint rate while playing (spectrum, marquee). Each frame costs CPU.
    pub animation_fps: u32,
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
            last_position_ms: None,
            show_eq: true,
            show_playlist: true,
            show_library: false,
            time_remaining: false,
            animation_fps: 30,
            eq: EqConfig::default(),
            window: WindowConfig::default(),
        }
    }
}

pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "khz-player")
}

/// One-time move of the pre-rename `rmp` config and data directories to
/// their `khz-player` locations. A directory is only moved when the new one
/// does not exist yet, so a fresh install or an already migrated setup is
/// untouched.
pub fn migrate_legacy_dirs() {
    let Some(new) = project_dirs() else { return };
    let Some(old) = directories::ProjectDirs::from("", "", "rmp") else {
        return;
    };
    for (from, to) in [
        (old.config_dir(), new.config_dir()),
        (old.data_dir(), new.data_dir()),
    ] {
        if !from.is_dir() || to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::rename(from, to) {
            Ok(()) => tracing::info!("moved {} to {}", from.display(), to.display()),
            Err(e) => tracing::warn!("could not move {} to {}: {e}", from.display(), to.display()),
        }
    }
}

pub fn config_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".khz-player"))
}

pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".khz-player"))
}

impl Config {
    pub fn path() -> PathBuf {
        config_dir().join("config.toml")
    }

    /// Loads the config, or defaults plus an error message for the user when
    /// the file could not be parsed.
    pub fn load() -> (Self, Option<String>) {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> (Self, Option<String>) {
        let (mut cfg, err) = match std::fs::read_to_string(path) {
            Ok(s) => match toml::from_str(&s) {
                Ok(c) => (c, None),
                Err(e) => (Self::default(), Some(set_aside(path, &e))),
            },
            Err(_) => (Self::default(), None),
        };
        cfg.clamp();
        (cfg, err)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, path: &Path) -> anyhow::Result<()> {
        write_atomic(path, &toml::to_string_pretty(self)?)
    }

    /// Pulls hand-edited values back into range.
    pub fn clamp(&mut self) {
        let d = Self::default();
        self.volume = in_range(self.volume, 0.0, 1.0, d.volume);
        self.balance = in_range(self.balance, -1.0, 1.0, 0.0);
        self.animation_fps = self.animation_fps.clamp(10, 60);
        let max = crate::eq_presets::MAX_DB;
        self.eq.preamp = in_range(self.eq.preamp, -max, max, 0.0);
        for b in &mut self.eq.bands {
            *b = in_range(*b, -max, max, 0.0);
        }
        self.window.h = self.window.h.filter(|h| h.is_finite() && *h > 0.0);
    }
}

fn in_range(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { fallback }
}

/// Renames a file that failed to parse to `<name>.broken`, so the next save
/// does not overwrite what the user wrote. Returns a message for the user.
pub fn set_aside(path: &Path, err: &dyn std::fmt::Display) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut broken = path.as_os_str().to_owned();
    broken.push(".broken");
    match std::fs::rename(path, &broken) {
        Ok(()) => format!("{name} could not be read, moved to {name}.broken: {err}"),
        Err(e) => format!("{name} could not be read ({err}); moving it aside failed: {e}"),
    }
}

/// Writes through a temporary file and a rename, so a crash never leaves a
/// half-written file behind.
pub fn write_atomic(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

/// A fresh empty directory for tests.
#[cfg(test)]
pub fn test_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("khz-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
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

    #[test]
    fn bad_file_is_renamed_not_overwritten() {
        let dir = test_dir("config-bad");
        let path = dir.join("config.toml");
        let text = "library_roots = [\"/my/music\"]\nvolume = oops\n";
        std::fs::write(&path, text).unwrap();
        let (cfg, err) = Config::load_from(&path);
        assert!(err.is_some_and(|e| e.contains("config.toml.broken")));
        assert_eq!(cfg.volume, Config::default().volume);
        cfg.save_to(&path).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("config.toml.broken")).unwrap(), text);
        let (_, err) = Config::load_from(&path);
        assert!(err.is_none());
    }

    #[test]
    fn values_clamped_on_load() {
        let dir = test_dir("config-clamp");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "volume = 3.0\nbalance = -7.0\nanimation_fps = 500\n[eq]\npreamp = 40.0\nbands = [-50,0,0,0,0,0,0,0,0,nan]\n",
        )
        .unwrap();
        let (c, err) = Config::load_from(&path);
        assert!(err.is_none());
        assert_eq!(c.volume, 1.0);
        assert_eq!(c.balance, -1.0);
        assert_eq!(c.animation_fps, 60);
        assert_eq!(c.eq.preamp, 12.0);
        assert_eq!(c.eq.bands[0], -12.0);
        assert_eq!(c.eq.bands[9], 0.0);
    }
}
