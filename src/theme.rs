//! Colors: Omarchy `colors.toml` when available, classic Winamp palette otherwise.

use std::path::PathBuf;

use egui::{Color32, CornerRadius, Stroke, Visuals};

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub bg: Color32,
    pub bg_dark: Color32,
    pub bg_darker: Color32,
    pub panel: Color32,
    pub frame: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_bright: Color32,
    pub digits: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub muted: Color32,
    pub spectrum_lo: Color32,
    pub spectrum_hi: Color32,
    pub peak: Color32,
    pub warn: Color32,
    pub dark: bool,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Theme {
    pub fn winamp() -> Self {
        Self {
            name: "winamp".into(),
            bg: hex(0x000000),
            bg_dark: hex(0x000000),
            bg_darker: hex(0x000000),
            panel: hex(0x1e2a3a),
            frame: hex(0x3b4b6b),
            text: hex(0x00ff00),
            text_dim: hex(0x00a000),
            text_bright: hex(0xffffff),
            digits: hex(0x00ff00),
            accent: hex(0x00ff00),
            selection: hex(0x0000c6),
            muted: hex(0x5a6a8a),
            spectrum_lo: hex(0x00ff00),
            spectrum_hi: hex(0xffff00),
            peak: hex(0xc0c0c0),
            warn: hex(0xff4040),
            dark: true,
        }
    }

    /// Parses Omarchy `colors.toml`. Non-hex values are skipped and each missing
    /// role falls back to the Winamp palette.
    pub fn from_omarchy(text: &str, name: &str) -> anyhow::Result<Self> {
        let table: toml::Table = toml::from_str(text)?;
        let get = |key: &str| -> Option<Color32> {
            table.get(key).and_then(|v| v.as_str()).and_then(parse_hex)
        };
        let mut t = Self::winamp();
        t.name = name.to_string();
        let mut found = 0;
        let mut set = |dst: &mut Color32, keys: &[&str]| {
            if let Some(c) = keys.iter().find_map(|k| get(k)) {
                *dst = c;
                found += 1;
            }
        };
        set(&mut t.bg, &["background"]);
        set(&mut t.bg_dark, &["dark_background", "background"]);
        set(&mut t.bg_darker, &["darker_background", "dark_background", "background"]);
        set(&mut t.panel, &["lighter_background", "selection"]);
        set(&mut t.frame, &["muted", "color8"]);
        set(&mut t.text, &["foreground"]);
        set(&mut t.text_dim, &["dark_foreground", "muted", "color8"]);
        set(&mut t.text_bright, &["bright_foreground", "light_foreground", "foreground"]);
        set(&mut t.digits, &["accent", "foreground"]);
        set(&mut t.accent, &["accent", "blue", "color4"]);
        set(&mut t.selection, &["selection", "selection_background"]);
        set(&mut t.muted, &["muted", "color8"]);
        set(&mut t.spectrum_lo, &["green", "color2"]);
        set(&mut t.spectrum_hi, &["bright_yellow", "yellow", "color11"]);
        set(&mut t.peak, &["light_foreground", "foreground"]);
        set(&mut t.warn, &["red", "color1"]);
        if found == 0 {
            anyhow::bail!("no usable colors");
        }
        t.dark = match table.get("mode").and_then(|v| v.as_str()) {
            Some("light") => false,
            Some(_) => true,
            None => luminance(t.bg) < 0.5,
        };
        Ok(t)
    }

    pub fn omarchy_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| {
            b.home_dir()
                .join(".local/state/omarchy/current")
        })
    }

    /// Loads the current Omarchy theme, or the Winamp palette.
    pub fn load() -> Self {
        let Some(dir) = Self::omarchy_dir() else {
            return Self::winamp();
        };
        let path = dir.join("theme/colors.toml");
        let name = std::fs::read_to_string(dir.join("theme.name"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "omarchy".into());
        match std::fs::read_to_string(&path) {
            Ok(text) => match Self::from_omarchy(&text, &name) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("theme {}: {e}; using Winamp colors", path.display());
                    Self::winamp()
                }
            },
            Err(_) => Self::winamp(),
        }
    }

    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        let r = CornerRadius::ZERO;
        v.override_text_color = Some(self.text);
        v.panel_fill = self.bg;
        v.window_fill = self.bg_dark;
        v.window_stroke = Stroke::new(1.0, self.frame);
        v.window_corner_radius = r;
        v.menu_corner_radius = r;
        v.extreme_bg_color = self.bg_darker;
        v.faint_bg_color = self.bg_dark;
        v.code_bg_color = self.bg_dark;
        v.hyperlink_color = self.accent;
        v.warn_fg_color = self.warn;
        v.error_fg_color = self.warn;
        v.selection.bg_fill = self.selection;
        v.selection.stroke = Stroke::new(1.0, self.text_bright);
        v.striped = false;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.bg;
        w.noninteractive.weak_bg_fill = self.bg;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.frame);
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text);
        w.inactive.bg_fill = self.panel;
        w.inactive.weak_bg_fill = self.panel;
        w.inactive.bg_stroke = Stroke::new(1.0, self.frame);
        w.inactive.fg_stroke = Stroke::new(1.0, self.text);
        w.hovered.bg_fill = self.selection;
        w.hovered.weak_bg_fill = self.selection;
        w.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        w.hovered.fg_stroke = Stroke::new(1.0, self.text_bright);
        w.active.bg_fill = self.accent;
        w.active.weak_bg_fill = self.accent;
        w.active.bg_stroke = Stroke::new(1.0, self.text_bright);
        w.active.fg_stroke = Stroke::new(1.0, self.bg_darker);
        w.open.bg_fill = self.panel;
        w.open.weak_bg_fill = self.panel;
        w.open.bg_stroke = Stroke::new(1.0, self.accent);
        w.open.fg_stroke = Stroke::new(1.0, self.text_bright);
        for wv in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.open,
        ] {
            wv.corner_radius = r;
            wv.expansion = 0.0;
        }
        v
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let visuals = self.visuals();
        ctx.all_styles_mut(|s| {
            s.visuals = visuals.clone();
            s.spacing.item_spacing = egui::vec2(4.0, 3.0);
            s.spacing.button_padding = egui::vec2(5.0, 1.0);
            s.spacing.interact_size.y = 18.0;
            s.spacing.slider_rail_height = 4.0;
            s.spacing.window_margin = egui::Margin::same(4);
            s.spacing.menu_margin = egui::Margin::same(3);
        });
    }
}

fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.trim().strip_prefix('#')?;
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    match h.len() {
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

fn luminance(c: Color32) -> f32 {
    (0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32) / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const OSAKA: &str = r##"
mode = "dark"
accent = "#509475"
selection = "#32473B"
muted = "#53685B"
background = "#111c18"
dark_background = "#0c1512"
darker_background = "#090f0d"
lighter_background = "#23372B"
foreground = "#C1C497"
dark_foreground = "#81B8A8"
light_foreground = "#D6D5BC"
bright_foreground = "#F7E8B2"
red = "#FF5345"
green = "#549e6a"
bright_yellow = "#E5C736"
hyprland_active_border = "rgba(509475ee) 45deg"
"##;

    #[test]
    fn parses_osaka_jade() {
        let t = Theme::from_omarchy(OSAKA, "osaka-jade").unwrap();
        assert_eq!(t.bg, hex(0x111c18));
        assert_eq!(t.accent, hex(0x509475));
        assert_eq!(t.digits, hex(0x509475));
        assert_eq!(t.spectrum_hi, hex(0xE5C736));
        assert_eq!(t.text_bright, hex(0xF7E8B2));
        assert!(t.dark);
    }

    #[test]
    fn partial_falls_back_per_role() {
        let t = Theme::from_omarchy("background = \"#102030\"\nfoo = \"rgba(1,2,3)\"\n", "x")
            .unwrap();
        assert_eq!(t.bg, hex(0x102030));
        assert_eq!(t.text, Theme::winamp().text);
    }

    #[test]
    fn garbage_fails() {
        assert!(Theme::from_omarchy("not = [toml", "x").is_err());
        assert!(Theme::from_omarchy("a = 1\n", "x").is_err());
    }

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#ff0000"), Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(parse_hex("#ff000080").map(|c| c.a()), Some(128));
        assert_eq!(parse_hex("ff0000"), None);
        assert_eq!(parse_hex("#ff00"), None);
        assert_eq!(parse_hex("#ééé"), None);
    }
}
