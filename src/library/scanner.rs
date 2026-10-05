//! Background library scan: walk roots, read changed files, prune missing ones.

use std::path::PathBuf;

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

fn scan(
    roots: &[PathBuf],
    db_path: &std::path::Path,
    send: &dyn Fn(ScanEvent),
) -> anyhow::Result<ScanEvent> {
    let started = std::time::Instant::now();
    let mut conn = db::open(db_path)?;
    let mut files: Vec<PathBuf> = roots
        .iter()
        .flat_map(|r| WalkDir::new(r).follow_links(true).into_iter())
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && tags::is_audio(e.path()))
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files.dedup();
    let total = files.len();
    send(ScanEvent::Progress { done: 0, total });

    let known = db::stamps(&conn)?;
    db::clear_seen(&conn, roots)?;
    let mut updated = 0;
    for (i, batch) in files.chunks(BATCH).enumerate() {
        let tx = conn.transaction()?;
        for path in batch {
            let stamp = tags::file_stamp(path);
            if known.get(path) == Some(&stamp) {
                db::mark_seen(&tx, path)?;
            } else {
                db::upsert_track(&tx, &tags::read(path))?;
                updated += 1;
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
        "library scan: {total} files, {updated} updated, {removed} removed in {:.1}s",
        started.elapsed().as_secs_f32()
    );
    Ok(ScanEvent::Done {
        tracks,
        updated,
        removed,
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
            } => (tracks.len(), updated, removed),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn incremental_rescan() {
        let dir = std::env::temp_dir().join(format!("rmp-scan-{}", std::process::id()));
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
}
