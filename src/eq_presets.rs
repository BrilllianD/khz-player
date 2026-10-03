use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{BANDS, config_dir};

pub const MAX_DB: f32 = 12.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub preamp: f32,
    pub bands: [f32; BANDS],
}

/// Classic Winamp presets as shipped by XMMS/Audacious (already scaled to ±12 dB).
const BUILTIN: &[(&str, [f32; BANDS])] = &[
    ("Classical", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -7.2, -7.2, -7.2, -9.6]),
    ("Club", [0.0, 0.0, 8.0, 5.6, 5.6, 5.6, 3.2, 0.0, 0.0, 0.0]),
    ("Dance", [9.6, 7.2, 2.4, 0.0, 0.0, -5.6, -7.2, -7.2, 0.0, 0.0]),
    ("Flat", [0.0; BANDS]),
    ("Full Bass", [9.6, 9.6, 9.6, 5.6, 1.6, -4.0, -8.0, -10.4, -11.2, -11.2]),
    ("Full Bass & Treble", [7.2, 5.6, 0.0, -7.2, -4.8, 1.6, 8.0, 11.2, 12.0, 12.0]),
    ("Full Treble", [-9.6, -9.6, -9.6, -4.0, 2.4, 11.2, 12.0, 12.0, 12.0, 12.0]),
    ("Laptop Speakers/Headphones", [4.8, 11.2, 5.6, -3.2, -2.4, 1.6, 4.8, 9.6, 12.0, 12.0]),
    ("Large Hall", [10.4, 10.4, 5.6, 5.6, 0.0, -4.8, -4.8, -4.8, 0.0, 0.0]),
    ("Live", [-4.8, 0.0, 4.0, 5.6, 5.6, 5.6, 4.0, 2.4, 2.4, 2.4]),
    ("Party", [7.2, 7.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 7.2, 7.2]),
    ("Pop", [-1.6, 4.8, 7.2, 8.0, 5.6, 0.0, -2.4, -2.4, -1.6, -1.6]),
    ("Reggae", [0.0, 0.0, 0.0, -5.6, 0.0, 6.4, 6.4, 0.0, 0.0, 0.0]),
    ("Rock", [8.0, 4.8, -5.6, -8.0, -3.2, 4.0, 8.8, 11.2, 11.2, 11.2]),
    ("Ska", [-2.4, -4.8, -4.0, 0.0, 4.0, 5.6, 8.8, 9.6, 11.2, 9.6]),
    ("Soft", [4.8, 1.6, 0.0, -2.4, 0.0, 4.0, 8.0, 9.6, 11.2, 12.0]),
    ("Soft Rock", [4.0, 4.0, 2.4, 0.0, -4.0, -5.6, -3.2, 0.0, 2.4, 8.8]),
    ("Techno", [8.0, 5.6, 0.0, -5.6, -4.8, 0.0, 8.0, 9.6, 9.6, 8.8]),
];

pub fn builtin() -> Vec<Preset> {
    BUILTIN
        .iter()
        .map(|(name, bands)| Preset {
            name: (*name).to_string(),
            preamp: 0.0,
            bands: bands.map(|b| b.clamp(-MAX_DB, MAX_DB)),
        })
        .collect()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PresetFile {
    #[serde(default)]
    preset: Vec<Preset>,
}

pub fn user_presets_path() -> PathBuf {
    config_dir().join("eq_presets.toml")
}

pub fn parse_user(text: &str) -> anyhow::Result<Vec<Preset>> {
    let f: PresetFile = toml::from_str(text)?;
    Ok(f.preset
        .into_iter()
        .map(|mut p| {
            p.preamp = p.preamp.clamp(-MAX_DB, MAX_DB);
            p.bands = p.bands.map(|b| b.clamp(-MAX_DB, MAX_DB));
            p
        })
        .collect())
}

pub fn render_user(presets: &[Preset]) -> anyhow::Result<String> {
    Ok(toml::to_string_pretty(&PresetFile {
        preset: presets.to_vec(),
    })?)
}

pub fn load_user() -> Vec<Preset> {
    let path = user_presets_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => parse_user(&s).unwrap_or_else(|e| {
            tracing::warn!("bad presets file {}: {e}", path.display());
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

pub fn save_user(presets: &[Preset]) -> anyhow::Result<()> {
    let path = user_presets_path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, render_user(presets)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_in_range() {
        let b = builtin();
        assert!(b.len() >= 17);
        for p in &b {
            assert!(p.bands.iter().all(|v| v.abs() <= MAX_DB), "{}", p.name);
        }
        let rock = b.iter().find(|p| p.name == "Rock").unwrap();
        assert_eq!(rock.bands[0], 8.0);
    }

    #[test]
    fn user_roundtrip_and_clamp() {
        let p = Preset {
            name: "Mine".into(),
            preamp: -3.0,
            bands: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0],
        };
        let text = render_user(std::slice::from_ref(&p)).unwrap();
        assert_eq!(parse_user(&text).unwrap(), vec![p]);
        let clamped =
            parse_user("[[preset]]\nname = \"x\"\nbands = [30,0,0,0,0,0,0,0,0,-30]\n").unwrap();
        assert_eq!(clamped[0].bands[0], 12.0);
        assert_eq!(clamped[0].bands[9], -12.0);
    }
}
