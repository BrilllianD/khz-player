//! Picks a system Nerd Font (monospace) and installs it as the primary font.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

const CANDIDATES: &[&str] = &[
    "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
    "/usr/share/fonts/TTF/CaskaydiaMonoNerdFont-Regular.ttf",
    "/usr/share/fonts/TTF/JetBrainsMonoNLNerdFont-Regular.ttf",
];

fn fc_match() -> Option<PathBuf> {
    let out = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", "monospace"])
        .output()
        .ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let p = PathBuf::from(s.trim());
    p.is_file().then_some(p)
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes(s.try_into().ok()?))
}

/// Mirrors `FontRef::from_index(bytes, 0)` of read-fonts 0.41.0, the only fallible call in
/// `FontFace::new` of epaint 0.36.2, which panics on `Err`. An egui upgrade must re-check
/// `FontFace::new` and this function.
fn parseable(bytes: &[u8]) -> bool {
    const VERSIONS: [u32; 3] = [0x0001_0000, 0x4F54_544F, 0x7472_7565];
    let sfnt_at = |off: usize| {
        off.checked_add(12).is_some_and(|end| end <= bytes.len())
            && be_u32(bytes, off).is_some_and(|v| VERSIONS.contains(&v))
    };
    if bytes.len() >= 12 && &bytes[..4] == b"ttcf" {
        // Collection: no fallback to the single-font branch.
        let Some(n) = be_u32(bytes, 8).filter(|&n| n >= 1) else {
            return false;
        };
        let offsets_end = (n as usize).checked_mul(4).and_then(|l| l.checked_add(12));
        if offsets_end.is_none_or(|end| end > bytes.len()) {
            return false;
        }
        return be_u32(bytes, 12).is_some_and(|off| sfnt_at(off as usize));
    }
    sfnt_at(0)
}

/// Reads `path` and returns its bytes when egui can load them, else the reason text.
fn load(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if parseable(&bytes) {
        Ok(bytes)
    } else {
        Err("not a TrueType/OpenType font".into())
    }
}

/// `Some("Rejected: <reason>")` when `path` cannot be installed, `None` when it can.
pub fn rejection(path: &Path) -> Option<String> {
    load(path).err().map(|r| format!("Rejected: {r}"))
}

fn find(configured: Option<&Path>) -> Option<(PathBuf, Vec<u8>)> {
    let sources = configured
        .map(Path::to_path_buf)
        .into_iter()
        .chain(CANDIDATES.iter().map(PathBuf::from))
        .filter(|p| p.is_file());
    let fc = || fc_match().into_iter();
    sources.chain(fc()).find_map(|p| match load(&p) {
        Ok(bytes) => Some((p, bytes)),
        Err(r) => {
            tracing::warn!("font {}: {r}", p.display());
            None
        }
    })
}

/// Returns true when a Nerd Font was installed (icon glyphs available).
pub fn install(ctx: &egui::Context, configured: Option<&Path>) -> bool {
    let mut defs = FontDefinitions::default();
    let mut nerd = false;
    if let Some((path, bytes)) = find(configured) {
        defs.font_data
            .insert("system-mono".into(), Arc::new(FontData::from_owned(bytes)));
        for fam in [FontFamily::Monospace, FontFamily::Proportional] {
            defs.families
                .entry(fam)
                .or_default()
                .insert(0, "system-mono".into());
        }
        nerd = path.to_string_lossy().contains("Nerd");
        tracing::info!("font: {}", path.display());
    }
    ctx.set_fonts(defs);
    nerd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_dir;

    #[test]
    fn rejects_text_file_and_install_does_not_panic() {
        let dir = test_dir("font-bad");
        let p = dir.join("bad.ttf");
        std::fs::write(&p, "not a font").unwrap();
        assert_eq!(
            rejection(&p).as_deref(),
            Some("Rejected: not a TrueType/OpenType font")
        );
        assert_ne!(find(Some(&p)).map(|(q, _)| q), Some(p.clone()));
        let ctx = egui::Context::default();
        install(&ctx, Some(&p));
        ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label("khz");
        })
        .drop_without_applying_deltas();
    }

    #[test]
    fn accepts_egui_default_fonts() {
        for (name, d) in &FontDefinitions::default().font_data {
            assert!(parseable(&d.font), "{name}");
        }
    }

    fn ttc(num: u32, offsets: &[u32], tail: &[u8]) -> Vec<u8> {
        let mut v = b"ttcf".to_vec();
        v.extend([0, 1, 0, 0]);
        v.extend(num.to_be_bytes());
        for o in offsets {
            v.extend(o.to_be_bytes());
        }
        v.extend(tail);
        v
    }

    #[test]
    fn byte_check_mirrors_from_index() {
        let sfnt = |tag: &[u8]| [tag, &[0u8; 8]].concat();
        assert!(parseable(&sfnt(b"OTTO")));
        assert!(parseable(&sfnt(&[0, 1, 0, 0])));
        assert!(parseable(&sfnt(b"true")));
        assert!(!parseable(b"OTTO\0\0\0\0\0\0\0"));
        assert!(!parseable(&sfnt(b"wOFF")));
        assert!(!parseable(&ttc(0, &[], &[0; 12])));
        assert!(!parseable(&ttc(2, &[20], &sfnt(&[0, 1, 0, 0]))));
        assert!(!parseable(&ttc(1, &[1000], &[0; 12])));
        assert!(!parseable(&ttc(1, &[20], &[0, 1, 0, 0, 0, 0])));
        assert!(!parseable(&ttc(1, &[16], &sfnt(b"ttcf"))));
        assert!(parseable(&ttc(1, &[16], &sfnt(&[0, 1, 0, 0]))));
    }

    #[test]
    fn rejection_reports_missing_file() {
        let p = test_dir("font-missing").join("none.ttf");
        assert!(rejection(&p).unwrap().starts_with("Rejected: "));
    }
}
