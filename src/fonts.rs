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

fn find(configured: Option<&Path>) -> Option<PathBuf> {
    configured
        .filter(|p| p.is_file())
        .map(Path::to_path_buf)
        .or_else(|| {
            CANDIDATES
                .iter()
                .map(PathBuf::from)
                .find(|p| p.is_file())
        })
        .or_else(fc_match)
}

/// Returns true when a Nerd Font was installed (icon glyphs available).
pub fn install(ctx: &egui::Context, configured: Option<&Path>) -> bool {
    let mut defs = FontDefinitions::default();
    let mut nerd = false;
    if let Some(path) = find(configured) {
        match std::fs::read(&path) {
            Ok(bytes) => {
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
            Err(e) => tracing::warn!("font {}: {e}", path.display()),
        }
    }
    ctx.set_fonts(defs);
    nerd
}
