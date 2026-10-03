use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{BANDS, config_dir, set_aside, write_atomic};

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

/// Loads user presets; an unparsable file is moved aside and reported.
pub fn load_user() -> (Vec<Preset>, Option<String>) {
    load_user_from(&user_presets_path())
}

pub fn load_user_from(path: &Path) -> (Vec<Preset>, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(s) => match parse_user(&s) {
            Ok(p) => (p, None),
            Err(e) => (Vec::new(), Some(set_aside(path, &e))),
        },
        Err(_) => (Vec::new(), None),
    }
}

pub fn save_user(presets: &[Preset]) -> anyhow::Result<()> {
    save_user_to(&user_presets_path(), presets)
}

pub fn save_user_to(path: &Path, presets: &[Preset]) -> anyhow::Result<()> {
    write_atomic(path, &render_user(presets)?)
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

    #[test]
    fn bad_file_renamed_and_save_is_atomic() {
        let dir = crate::config::test_dir("presets-bad");
        let path = dir.join("eq_presets.toml");
        std::fs::write(&path, "[[preset]]\nname = 1\n").unwrap();
        let (presets, err) = load_user_from(&path);
        assert!(presets.is_empty());
        assert!(err.is_some());
        assert!(dir.join("eq_presets.toml.broken").exists());

        let p = Preset { name: "Mine".into(), preamp: 0.0, bands: [1.0; BANDS] };
        save_user_to(&path, std::slice::from_ref(&p)).unwrap();
        assert!(!dir.join("eq_presets.toml.tmp").exists());
        assert_eq!(load_user_from(&path), (vec![p], None));
    }
}
