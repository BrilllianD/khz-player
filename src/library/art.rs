//! Cover lookup: embedded picture (cached by content hash) or a cover file next to the track.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use lofty::config::{ParseOptions, ParsingMode};
use lofty::picture::{Picture, PictureType};
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::Tag;

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Cover of `track`: embedded front cover, else the first embedded JPEG/PNG (written once to
/// the cache), else `cover|folder|front.(jpg|jpeg|png)` in the track's directory.
pub fn find(track: &Path) -> Option<PathBuf> {
    find_in(track, &cache_dir())
}

pub fn cache_dir() -> PathBuf {
    crate::config::project_dirs()
        .map(|d| d.cache_dir().join("art"))
        .unwrap_or_else(|| std::env::temp_dir().join("khz-player-art"))
}

/// Removes every cached cover; a missing cache dir is fine.
pub fn clear_cache() -> std::io::Result<()> {
    clear_dir(&cache_dir())
}

fn find_in(track: &Path, cache: &Path) -> Option<PathBuf> {
    embedded(track)
        .and_then(|d| cache_picture(&d, cache))
        .or_else(|| folder_cover(track_dir(track)?))
}

/// Folder of `track`; a bare relative path has an empty parent, which is the current dir.
fn track_dir(track: &Path) -> Option<&Path> {
    let dir = track.parent()?;
    Some(if dir.as_os_str().is_empty() { Path::new(".") } else { dir })
}

fn embedded(track: &Path) -> Option<Vec<u8>> {
    let opts = ParseOptions::new()
        .parsing_mode(ParsingMode::Relaxed)
        .read_cover_art(true)
        .read_properties(false)
        .max_junk_bytes(64 * 1024);
    let tagged = match Probe::open(track).and_then(|p| p.options(opts).read()) {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!("cover probe {}: {e}", track.display());
            return None;
        }
    };
    pick_picture(tagged.tags()).map(|p| p.data().to_vec())
}

fn pick_picture(tags: &[Tag]) -> Option<&Picture> {
    let pics = || {
        tags.iter()
            .flat_map(|t| t.pictures())
            .filter(|p| image_ext(p.data()).is_some())
    };
    pics()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pics().next())
}

fn image_ext(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else {
        None
    }
}

/// FNV-1a 64: stable across Rust releases, so cache names survive upgrades.
fn fnv1a64(data: &[u8]) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

fn cache_picture(data: &[u8], cache: &Path) -> Option<PathBuf> {
    let ext = image_ext(data)?;
    let hash = fnv1a64(data);
    let path = cache.join(format!("{hash:016x}.{ext}"));
    if path.is_file() {
        return Some(path);
    }
    let n = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = cache.join(format!(".{hash:016x}.{}.{n}.tmp", std::process::id()));
    let res = fs::create_dir_all(cache)
        .and_then(|_| fs::write(&tmp, data))
        .and_then(|_| fs::rename(&tmp, &path));
    match res {
        Ok(()) => Some(path),
        Err(e) => {
            tracing::debug!("cover cache {}: {e}", path.display());
            let _ = fs::remove_file(&tmp);
            None
        }
    }
}

fn folder_cover(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<((usize, String), PathBuf)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_lowercase) else {
            continue;
        };
        let Some((stem, ext)) = name.rsplit_once('.') else {
            continue;
        };
        let Some(rank) = ["cover", "folder", "front"].iter().position(|s| *s == stem) else {
            continue;
        };
        if !matches!(ext, "jpg" | "jpeg" | "png") {
            continue;
        }
        let key = (rank, name);
        if best.as_ref().is_none_or(|(k, _)| key < *k) {
            best = Some((key, path));
        }
    }
    best.map(|(_, p)| p)
}

fn clear_dir(dir: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::tag::TagType;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("khz-art-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";
    const JPG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];

    fn pic(bytes: &[u8], t: PictureType) -> Picture {
        Picture::unchecked(bytes.to_vec()).pic_type(t).build()
    }

    #[test]
    fn fnv1a64_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
    }

    #[test]
    fn image_ext_from_magic() {
        assert_eq!(image_ext(PNG), Some("png"));
        assert_eq!(image_ext(JPG), Some("jpg"));
        assert_eq!(image_ext(b"GIF89a"), None);
    }

    #[test]
    fn pick_prefers_front_cover() {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.push_picture(pic(JPG, PictureType::Other));
        tag.push_picture(pic(PNG, PictureType::CoverFront));
        let tags = [tag];
        assert_eq!(pick_picture(&tags).unwrap().data(), PNG);

        let mut tag = Tag::new(TagType::Id3v2);
        tag.push_picture(pic(JPG, PictureType::Other));
        tag.push_picture(pic(b"GIF89a", PictureType::CoverFront));
        let tags = [tag];
        assert_eq!(pick_picture(&tags).unwrap().data(), JPG);

        assert!(pick_picture(&[Tag::new(TagType::Id3v2)]).is_none());
    }

    #[test]
    fn cache_name_is_hash_and_ext() {
        let dir = tmp("cache").join("sub");
        let p = cache_picture(JPG, &dir).unwrap();
        assert_eq!(p, dir.join(format!("{:016x}.jpg", fnv1a64(JPG))));
        assert_eq!(fs::read(&p).unwrap(), JPG);
        assert_eq!(cache_picture(JPG, &dir), Some(p));
        let names: Vec<_> = fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(names.len(), 1);
        assert!(cache_picture(b"not an image", &dir).is_none());
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn folder_fallback_order_and_case() {
        let dir = tmp("folder");
        for f in ["Front.PNG", "Folder.jpg", "cover.txt", "back.jpg", "track.mp3"] {
            fs::write(dir.join(f), b"xx").unwrap();
        }
        let track = dir.join("track.mp3");
        let cache = dir.join("cache");
        assert_eq!(find_in(&track, &cache), Some(dir.join("Folder.jpg")));
        fs::write(dir.join("COVER.jpeg"), b"xx").unwrap();
        assert_eq!(find_in(&track, &cache), Some(dir.join("COVER.jpeg")));
        fs::remove_dir_all(&dir).unwrap();

        let dir = tmp("folder-none");
        fs::write(dir.join("back.jpg"), b"xx").unwrap();
        fs::write(dir.join("track.mp3"), b"xx").unwrap();
        assert_eq!(find_in(&dir.join("track.mp3"), &dir.join("cache")), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bare_relative_track_uses_current_dir() {
        assert_eq!(track_dir(Path::new("track.mp3")), Some(Path::new(".")));
        assert_eq!(track_dir(Path::new("a/track.mp3")), Some(Path::new("a")));
        assert_eq!(track_dir(Path::new("/track.mp3")), Some(Path::new("/")));
        assert_eq!(track_dir(Path::new("/")), None);
    }

    #[test]
    fn clear_dir_ok_when_missing() {
        let dir = tmp("clear");
        fs::write(dir.join("a.jpg"), b"x").unwrap();
        clear_dir(&dir).unwrap();
        assert!(!dir.exists());
        clear_dir(&dir).unwrap();
    }
}
