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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplayGainMode {
    #[default]
    Off,
    Track,
    Album,
    /// Album gain, track gain while shuffling.
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visualization {
    #[default]
    Bars,
    Scope,
    Off,
}

impl Visualization {
    pub fn next(self) -> Self {
        match self {
            Self::Bars => Self::Scope,
            Self::Scope => Self::Off,
            Self::Off => Self::Bars,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplayGainConfig {
    pub mode: ReplayGainMode,
    /// Added to the chosen gain.
    pub preamp_db: f32,
    /// Gain for tracks without a usable ReplayGain tag.
    pub untagged_db: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub x: Option<f32>,
    pub y: Option<f32>,
    /// Width of the main column (player, EQ, playlist), without the library panel.
    pub w: Option<f32>,
    pub h: Option<f32>,
    /// Width of the library panel.
    pub library_w: Option<f32>,
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
    /// Use the current Omarchy theme and follow its changes; false = Winamp colours.
    pub follow_omarchy_theme: bool,
    /// Repaint rate while playing (spectrum, marquee). Each frame costs CPU.
    pub animation_fps: u32,
    /// Announce the next track to the engine so it starts without a gap.
    pub gapless: bool,
    /// Sleep timer length in minutes (Ctrl+S).
    pub sleep_minutes: u32,
    /// Desktop notification on each started track while the window is unfocused.
    pub notify_track_change: bool,
    /// Cover of the current track at the right of the spectrum (Settings, Appearance).
    pub show_cover: bool,
    /// Spectrum row: bars, scope or off; click the row to cycle.
    pub visualization: Visualization,
    /// UI at zoom 2.0 (Ctrl+D); window sizes stay saved at 1x.
    pub double_size: bool,
    /// Output device as a cpal `DeviceId` string (`pulseaudio:<sink>`); None = system default.
    pub output_device: Option<String>,
    pub eq: EqConfig,
    /// Loudness normalisation from the file tags (Settings, Audio).
    pub replaygain: ReplayGainConfig,
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
            follow_omarchy_theme: true,
            animation_fps: 30,
            gapless: true,
            sleep_minutes: 30,
            notify_track_change: false,
            show_cover: false,
            visualization: Visualization::Bars,
            double_size: false,
            output_device: None,
            eq: EqConfig::default(),
            replaygain: ReplayGainConfig::default(),
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
        self.sleep_minutes = self.sleep_minutes.clamp(1, 480);
        self.output_device = self.output_device.take().filter(|s| !s.trim().is_empty());
        let max = crate::eq_presets::MAX_DB;
        self.eq.preamp = in_range(self.eq.preamp, -max, max, 0.0);
        for b in &mut self.eq.bands {
            *b = in_range(*b, -max, max, 0.0);
        }
        self.replaygain.preamp_db = in_range(self.replaygain.preamp_db, -15.0, 15.0, 0.0);
        self.replaygain.untagged_db = in_range(self.replaygain.untagged_db, -15.0, 15.0, 0.0);
        for v in [&mut self.window.w, &mut self.window.h, &mut self.window.library_w] {
            *v = v.filter(|x| x.is_finite() && *x > 0.0);
        }
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
        c.output_device = Some("pulseaudio:alsa_output.x".into());
        let s = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.eq.bands[3], 4.5);
        assert_eq!(back.repeat, Repeat::One);
        assert_eq!(
            back.output_device.as_deref(),
            Some("pulseaudio:alsa_output.x")
        );
    }

    #[test]
    fn output_device_empty_loads_as_none() {
        let dir = test_dir("config-output-device");
        let path = dir.join("config.toml");
        std::fs::write(&path, "output_device = \"\"\n").unwrap();
        let (c, err) = Config::load_from(&path);
        assert!(err.is_none());
        assert_eq!(c.output_device, None);
        std::fs::write(&path, "output_device = \"pulseaudio:x\"\n").unwrap();
        let (c, _) = Config::load_from(&path);
        assert_eq!(c.output_device.as_deref(), Some("pulseaudio:x"));
    }

    #[test]
    fn old_config_follows_theme() {
        let c: Config = toml::from_str("volume = 0.5\n").unwrap();
        assert!(c.follow_omarchy_theme);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let c: Config = toml::from_str("volume = 0.3\n[eq]\nenabled = true\n").unwrap();
        assert_eq!(c.volume, 0.3);
        assert!(c.eq.enabled);
        assert!(c.show_playlist);
        assert!(c.gapless);
        assert_eq!(c.sleep_minutes, 30);
        assert!(!c.notify_track_change);
        assert!(!c.show_cover);
        assert_eq!(c.visualization, Visualization::Bars);
        assert!(!c.double_size);
        assert_eq!(c.output_device, None);
        assert_eq!(c.replaygain, ReplayGainConfig::default());
    }

    #[test]
    fn double_size_loads() {
        let c: Config = toml::from_str("double_size = true\n").unwrap();
        assert!(c.double_size);
    }

    #[test]
    fn visualization_loads_and_cycles() {
        let c: Config = toml::from_str("visualization = \"scope\"\n").unwrap();
        assert_eq!(c.visualization, Visualization::Scope);
        assert_eq!(Visualization::Bars.next(), Visualization::Scope);
        assert_eq!(Visualization::Bars.next().next().next(), Visualization::Bars);
    }

    #[test]
    fn show_cover_loads() {
        let c: Config = toml::from_str("show_cover = true\n").unwrap();
        assert!(c.show_cover);
    }

    #[test]
    fn notify_track_change_loads() {
        let c: Config = toml::from_str("notify_track_change = true\n").unwrap();
        assert!(c.notify_track_change);
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
            "volume = 3.0\nbalance = -7.0\nanimation_fps = 500\nsleep_minutes = 9999\n[eq]\npreamp = 40.0\nbands = [-50,0,0,0,0,0,0,0,0,nan]\n[replaygain]\nmode = \"album\"\npreamp_db = 40.0\nuntagged_db = -99.0\n",
        )
        .unwrap();
        let (c, err) = Config::load_from(&path);
        assert!(err.is_none());
        assert_eq!(c.volume, 1.0);
        assert_eq!(c.balance, -1.0);
        assert_eq!(c.animation_fps, 60);
        assert_eq!(c.sleep_minutes, 480);
        assert_eq!(c.eq.preamp, 12.0);
        assert_eq!(c.eq.bands[0], -12.0);
        assert_eq!(c.eq.bands[9], 0.0);
        assert_eq!(c.replaygain.mode, ReplayGainMode::Album);
        assert_eq!(c.replaygain.preamp_db, 15.0);
        assert_eq!(c.replaygain.untagged_db, -15.0);
    }

    #[test]
    fn sleep_minutes_zero_clamps_to_one() {
        let mut c: Config = toml::from_str("sleep_minutes = 0\n").unwrap();
        c.clamp();
        assert_eq!(c.sleep_minutes, 1);
    }

    #[test]
    fn bad_window_widths_load_as_none() {
        let dir = test_dir("config-window-w");
        let path = dir.join("config.toml");
        std::fs::write(&path, "[window]\nw = -5.0\nlibrary_w = nan\n").unwrap();
        let (c, err) = Config::load_from(&path);
        assert!(err.is_none());
        assert_eq!(c.window.w, None);
        assert_eq!(c.window.library_w, None);
    }
}
