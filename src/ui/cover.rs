//! Cover decoding off the UI thread and the egui texture that shows it.

use std::path::{Path, PathBuf};

use crossbeam_channel::{Receiver, Sender};
use egui::{ColorImage, TextureHandle, TextureOptions};

use crate::library::art;

/// Maximum side of a decoded cover, in pixels.
pub const COVER_PX: u32 = 256;

type Decoded = (PathBuf, Option<ColorImage>);

/// Reads a JPEG or PNG (format guessed by content), downscaled to at most
/// `max_px` per side with the aspect ratio kept. Never upscales.
pub fn decode(path: &Path, max_px: u32) -> Option<ColorImage> {
    let res = image::ImageReader::open(path)
        .map_err(image::ImageError::IoError)
        .and_then(|r| r.with_guessed_format().map_err(image::ImageError::IoError))
        .and_then(|r| r.decode());
    let mut img = match res {
        Ok(i) => i,
        Err(e) => {
            tracing::debug!("cover decode {}: {e}", path.display());
            return None;
        }
    };
    if img.width() > max_px || img.height() > max_px {
        img = img.thumbnail(max_px, max_px);
    }
    let (w, h) = (img.width() as usize, img.height() as usize);
    let rgba = img.into_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw()))
}

/// One egui texture that follows a wanted picture. Decoding runs on a
/// short-lived thread; stale results are dropped by key.
pub struct CoverTexture {
    name: &'static str,
    want: Option<PathBuf>,
    tex: Option<TextureHandle>,
    tx: Sender<Decoded>,
    rx: Receiver<Decoded>,
}

impl CoverTexture {
    pub fn new(name: &'static str) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self { name, want: None, tex: None, tx, rx }
    }

    /// `image` is an image file. Call every frame while the picture is wanted.
    pub fn sync(&mut self, ctx: &egui::Context, image: Option<&Path>) {
        self.sync_with(ctx, image, false);
    }

    /// `track` is an audio file; the worker looks its cover up first.
    /// Call every frame while the picture is wanted.
    pub fn sync_track(&mut self, ctx: &egui::Context, track: Option<&Path>) {
        self.sync_with(ctx, track, true);
    }

    pub fn texture(&self) -> Option<&TextureHandle> {
        self.tex.as_ref()
    }

    fn sync_with(&mut self, ctx: &egui::Context, key: Option<&Path>, find: bool) {
        if key != self.want.as_deref() {
            self.want = key.map(Path::to_path_buf);
            self.tex = None;
            if let Some(p) = self.want.clone() {
                let tx = self.tx.clone();
                let ctx = ctx.clone();
                let spawned = std::thread::Builder::new().name("cover-decode".into()).spawn(move || {
                    let img = if find {
                        art::find(&p).and_then(|c| decode(&c, COVER_PX))
                    } else {
                        decode(&p, COVER_PX)
                    };
                    let _ = tx.send((p, img));
                    ctx.request_repaint();
                });
                if let Err(e) = spawned {
                    tracing::warn!("cover decode thread: {e}");
                }
            }
        }
        for (p, img) in self.rx.try_iter() {
            if self.want.as_deref() == Some(p.as_path()) {
                self.tex = img.map(|i| ctx.load_texture(self.name, i, TextureOptions::LINEAR));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temp dir removed on drop.
    struct Tmp(PathBuf);

    impl std::ops::Deref for Tmp {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(name: &str) -> Tmp {
        let d = std::env::temp_dir().join(format!("khz-cover-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }

    fn png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let p = dir.join(name);
        image::RgbaImage::new(w, h).save(&p).unwrap();
        p
    }

    #[test]
    fn decode_downscales_keeping_aspect() {
        let d = tmp("big");
        let img = decode(&png(&d, "big.png", 300, 200), 64).unwrap();
        assert_eq!(img.size[0], 64);
        assert!((40..=44).contains(&img.size[1]), "{:?}", img.size);
    }

    #[test]
    fn decode_keeps_small_images() {
        let d = tmp("small");
        let img = decode(&png(&d, "s.png", 10, 8), 64).unwrap();
        assert_eq!(img.size, [10, 8]);
    }

    #[test]
    fn decode_rejects_non_images() {
        let d = tmp("bad");
        let f = d.join("x.jpg");
        std::fs::write(&f, b"not an image").unwrap();
        assert!(decode(&f, 64).is_none());
        assert!(decode(&d.join("missing.png"), 64).is_none());
    }

    #[test]
    fn texture_follows_wanted_path() {
        let d = tmp("tex");
        let p = png(&d, "c.png", 10, 8);
        let ctx = egui::Context::default();
        let mut c = CoverTexture::new("t");
        for _ in 0..200 {
            ctx.run_ui(Default::default(), |ui| c.sync(ui.ctx(), Some(&p)))
                .drop_without_applying_deltas();
            if c.texture().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(c.texture().unwrap().size(), [10, 8]);
        ctx.run_ui(Default::default(), |ui| c.sync(ui.ctx(), None))
            .drop_without_applying_deltas();
        assert!(c.texture().is_none());
    }
}
