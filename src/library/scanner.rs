//! Background library scan: walk roots, read changed files, prune missing ones.

use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;
use walkdir::WalkDir;

use super::{Track, db, tags};

const BATCH: usize = 200;

#[derive(Debug)]
pub enum ScanEvent {
    Progress { done: usize, total: usize },
    Done {
        tracks: Vec<Track>,
        updated: usize,
        removed: usize,
        /// Walk problems; the affected roots kept their tracks.
        errors: Vec<String>,
    },
    Error(String),
}

pub fn start(roots: Vec<PathBuf>, db_path: PathBuf, repaint: egui::Context) -> Receiver<ScanEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let spawned = std::thread::Builder::new()
        .name("library-scan".into())
        .spawn(move || {
            let send = |ev: ScanEvent| {
                let _ = tx.send(ev);
                repaint.request_repaint();
            };
            match scan(&roots, &db_path, &send) {
                Ok(ev) => send(ev),
                Err(e) => send(ScanEvent::Error(format!("{e:#}"))),
            }
        });
    if let Err(e) = spawned {
        tracing::error!("spawn scanner: {e}");
    }
    rx
}

/// Result of walking one library root.
struct RootWalk {
    files: Vec<PathBuf>,
    errors: Vec<String>,
}

impl RootWalk {
    /// A root that failed or looks empty (e.g. an unmounted drive) keeps its tracks.
    fn prunable(&self) -> bool {
        self.errors.is_empty() && !self.files.is_empty()
    }
}

fn walk_root(root: &Path) -> RootWalk {
    let mut walk = RootWalk {
        files: Vec::new(),
        errors: Vec::new(),
    };
    for entry in WalkDir::new(root).follow_links(true) {
        match entry {
            Ok(e) if e.file_type().is_file() && tags::is_audio(e.path()) => {
                walk.files.push(e.into_path());
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("scan walk: {e}");
                walk.errors.push(e.to_string());
            }
        }
    }
    walk
}

fn scan(
    roots: &[PathBuf],
    db_path: &std::path::Path,
    send: &dyn Fn(ScanEvent),
) -> anyhow::Result<ScanEvent> {
    let started = std::time::Instant::now();
    let mut conn = db::open(db_path)?;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut prune_roots: Vec<PathBuf> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for root in roots {
        let walk = walk_root(root);
        if walk.prunable() {
            prune_roots.push(root.clone());
        } else if walk.errors.is_empty() {
            errors.push(format!(
                "{}: no audio files found, kept its tracks",
                root.display()
            ));
        }
        errors.extend(walk.errors);
        files.extend(walk.files);
    }
    files.sort();
    files.dedup();
    let total = files.len();
    send(ScanEvent::Progress { done: 0, total });

    let known = db::stamps(&conn)?;
    db::clear_seen(&conn, &prune_roots)?;
    let mut updated = 0;
    // Tags are read before the transaction so the write lock is held only for the inserts.
    for (i, batch) in files.chunks(BATCH).enumerate() {
        let work: Vec<Option<Track>> = batch
            .iter()
            .map(|path| {
                if known.get(path) == Some(&tags::file_stamp(path)) {
                    None
                } else {
                    Some(tags::read(path))
                }
            })
            .collect();
        let tx = conn.transaction()?;
        for (path, track) in batch.iter().zip(&work) {
            match track {
                None => db::mark_seen(&tx, path)?,
                Some(t) => {
                    db::upsert_track(&tx, t)?;
                    updated += 1;
                }
            }
        }
        tx.commit()?;
        send(ScanEvent::Progress {
            done: (i * BATCH + batch.len()).min(total),
            total,
        });
    }
    let removed = db::delete_unseen(&conn)?;
    let tracks = db::load_tracks(&conn)?;
    tracing::info!(
        "library scan: {total} files, {updated} updated, {removed} removed, {} walk problems in {:.1}s",
        errors.len(),
        started.elapsed().as_secs_f32()
    );
    Ok(ScanEvent::Done {
        tracks,
        updated,
        removed,
        errors,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Silent 8 kHz mono 16-bit WAV.
    pub(crate) fn write_wav(path: &std::path::Path, frames: u32) {
        write_wav_rate(path, frames, 8000);
    }

    /// Silent mono 16-bit WAV at `rate` Hz.
    pub(crate) fn write_wav_rate(path: &std::path::Path, frames: u32, rate: u32) {
        let data_len = frames * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&1u16.to_le_bytes()); // mono
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes()); // byte rate
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.resize(b.len() + data_len as usize, 0);
        std::fs::write(path, b).unwrap();
    }

    /// Silent 8 kHz mono 16-bit AIFF (big-endian, 54-byte header).
    pub(crate) fn write_aiff(path: &std::path::Path, frames: u32) {
        let rate: u32 = 8000;
        let data_len = frames * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"FORM");
        b.extend_from_slice(&(46 + data_len).to_be_bytes());
        b.extend_from_slice(b"AIFFCOMM");
        b.extend_from_slice(&18u32.to_be_bytes());
        b.extend_from_slice(&1u16.to_be_bytes()); // mono
        b.extend_from_slice(&frames.to_be_bytes());
        b.extend_from_slice(&16u16.to_be_bytes());
        // Sample rate as an 80-bit IEEE extended float.
        let e = 31 - rate.leading_zeros();
        b.extend_from_slice(&(16383 + e as u16).to_be_bytes());
        b.extend_from_slice(&((rate as u64) << (63 - e)).to_be_bytes());
        b.extend_from_slice(b"SSND");
        b.extend_from_slice(&(8 + data_len).to_be_bytes());
        b.extend_from_slice(&[0; 8]); // offset, block size
        b.resize(b.len() + data_len as usize, 0);
        std::fs::write(path, b).unwrap();
    }

    fn run(root: &std::path::Path, db: &std::path::Path) -> (usize, usize, usize) {
        match scan(&[root.to_path_buf()], db, &|_| {}).unwrap() {
            ScanEvent::Done {
                tracks,
                updated,
                removed,
                ..
            } => (tracks.len(), updated, removed),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn non_utf8_file_name_is_scanned_once() {
        use std::os::unix::ffi::OsStrExt;
        let dir = crate::config::test_dir("scan-non-utf8");
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let db = dir.join("lib.db");
        write_wav(&music.join(std::ffi::OsStr::from_bytes(b"bad\xff.wav")), 8000);
        assert_eq!(run(&music, &db), (1, 1, 0));
        assert_eq!(run(&music, &db), (1, 0, 0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn incremental_rescan() {
        let dir = std::env::temp_dir().join(format!("khz-scan-{}", std::process::id()));
        let music = dir.join("music");
        std::fs::create_dir_all(music.join("sub")).unwrap();
        let db = dir.join("lib.db");
        write_wav(&music.join("a.wav"), 8000);
        write_wav(&music.join("sub/b.wav"), 16000);
        std::fs::write(music.join("notes.txt"), "x").unwrap();

        assert_eq!(run(&music, &db), (2, 2, 0));
        assert_eq!(run(&music, &db), (2, 0, 0));

        // Changed size => re-read only that file.
        write_wav(&music.join("a.wav"), 4000);
        assert_eq!(run(&music, &db), (2, 1, 0));

        std::fs::remove_file(music.join("sub/b.wav")).unwrap();
        assert_eq!(run(&music, &db), (1, 0, 1));

        let conn = db::open(&db).unwrap();
        let t = &db::load_tracks(&conn).unwrap()[0];
        assert_eq!(t.duration_ms, Some(500));
        assert_eq!(t.display_title(), "a");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn prunable_rules() {
        let f = || vec![PathBuf::from("a.wav")];
        let e = || vec!["boom".to_string()];
        assert!(RootWalk { files: f(), errors: vec![] }.prunable());
        assert!(!RootWalk { files: vec![], errors: vec![] }.prunable());
        assert!(!RootWalk { files: f(), errors: e() }.prunable());
    }

    #[test]
    fn walk_missing_root_errors() {
        let dir = crate::config::test_dir("walk-missing");
        let walk = walk_root(&dir.join("nope"));
        assert_eq!(walk.errors.len(), 1);
        assert!(!walk.prunable());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn run_roots(roots: &[PathBuf], db: &Path) -> (usize, usize, usize, usize) {
        match scan(roots, db, &|_| {}).unwrap() {
            ScanEvent::Done {
                tracks,
                updated,
                removed,
                errors,
            } => (tracks.len(), updated, removed, errors.len()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn emptied_root_keeps_tracks() {
        let dir = crate::config::test_dir("scan-emptied");
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let db = dir.join("lib.db");
        write_wav(&music.join("a.wav"), 8000);
        write_wav(&music.join("b.wav"), 8000);
        assert_eq!(run_roots(std::slice::from_ref(&music), &db), (2, 2, 0, 0));
        std::fs::remove_file(music.join("a.wav")).unwrap();
        std::fs::remove_file(music.join("b.wav")).unwrap();
        assert_eq!(run_roots(&[music], &db), (2, 0, 0, 1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_root_does_not_block_pruning_others() {
        let dir = crate::config::test_dir("scan-missing");
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let db = dir.join("lib.db");
        let roots = [music.clone(), dir.join("gone")];
        write_wav(&music.join("a.wav"), 8000);
        write_wav(&music.join("b.wav"), 8000);
        assert_eq!(run_roots(&roots, &db), (2, 2, 0, 1));
        std::fs::remove_file(music.join("b.wav")).unwrap();
        assert_eq!(run_roots(&roots, &db), (1, 0, 1, 1));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn interrupted_scan_does_not_lose_unmounted_root() {
        let dir = crate::config::test_dir("scan-interrupted");
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        let db = dir.join("lib.db");
        write_wav(&music.join("a.wav"), 8000);
        assert_eq!(run_roots(std::slice::from_ref(&music), &db), (1, 1, 0, 0));
        // An earlier scan stopped right after clear_seen.
        let conn = db::open(&db).unwrap();
        db::clear_seen(&conn, std::slice::from_ref(&music)).unwrap();
        drop(conn);
        std::fs::remove_dir_all(&music).unwrap();
        assert_eq!(run_roots(&[music], &db), (1, 0, 0, 1));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
