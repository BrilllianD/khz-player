use super::*;

impl App {
    pub fn push_eq(&self) {
        let e = &self.cfg.eq;
        self.audio.shared.set_eq(&crate::audio::dsp::EqParams {
            enabled: e.enabled,
            preamp_db: e.preamp,
            bands_db: e.bands,
        });
    }

    pub fn apply_preset(&mut self, p: &Preset) {
        self.cfg.eq.bands = p.bands;
        self.cfg.eq.preamp = p.preamp;
        self.cfg.eq.preset = p.name.clone();
        self.push_eq();
        self.mark_cfg();
        self.remember_auto(Some(&p.name));
    }

    /// With AUTO on, stores (or with `None` drops) the playing track's preset mapping.
    pub(super) fn remember_auto(&self, preset: Option<&str>) {
        if self.cfg.eq.auto
            && let (Some(db), Some(now)) = (&self.db, &self.now)
            && let Err(e) = db::eq_auto_set(db, &now.path, preset)
        {
            tracing::warn!("eq auto: {e}");
        }
    }

    /// A slider tweak: the curve no longer matches a named preset, so the
    /// track's mapping goes (one DB write per tweak sequence, not per frame).
    pub fn set_eq_curve(&mut self, preamp: f32, bands: [f32; crate::config::BANDS]) {
        let had_preset = !self.cfg.eq.preset.is_empty();
        self.cfg.eq.preamp = preamp;
        self.cfg.eq.bands = bands;
        self.cfg.eq.preset.clear();
        self.push_eq();
        self.mark_cfg();
        if had_preset {
            self.remember_auto(None);
        }
    }

    pub fn find_preset(&self, name: &str) -> Option<Preset> {
        self.user_presets
            .iter()
            .chain(&self.builtin_presets)
            .find(|p| p.name == name)
            .cloned()
    }

    pub fn save_user_preset(&mut self, name: String) {
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let p = Preset {
            name: name.clone(),
            preamp: self.cfg.eq.preamp,
            bands: self.cfg.eq.bands,
        };
        match self.user_presets.iter_mut().find(|u| u.name == name) {
            Some(u) => *u = p,
            None => self.user_presets.push(p),
        }
        self.cfg.eq.preset = name.clone();
        self.mark_cfg();
        self.remember_auto(Some(&name));
        if let Err(e) = eq_presets::save_user(&self.user_presets) {
            self.toast(format!("Cannot save presets: {e}"));
        }
    }

    pub fn delete_user_preset(&mut self, name: &str) {
        self.user_presets.retain(|p| p.name != name);
        if let Err(e) = eq_presets::save_user(&self.user_presets) {
            self.toast(format!("Cannot save presets: {e}"));
        }
        if !self.builtin_presets.iter().any(|p| p.name == name) {
            if let Some(db) = &self.db
                && let Err(e) = db::eq_auto_forget_preset(db, name)
            {
                tracing::warn!("eq auto: {e}");
            }
            if self.cfg.eq.preset == name {
                self.cfg.eq.preset.clear();
                self.mark_cfg();
            }
        }
    }

    pub(super) fn auto_eq_for(&mut self, path: &Path) {
        if !self.cfg.eq.auto {
            return;
        }
        let name = self
            .db
            .as_ref()
            .and_then(|db| db::eq_auto_get(db, path).ok().flatten());
        if let Some(p) = name.and_then(|n| self.find_preset(&n)) {
            self.cfg.eq.bands = p.bands;
            self.cfg.eq.preamp = p.preamp;
            self.cfg.eq.preset = p.name;
            self.push_eq();
        }
    }
}
