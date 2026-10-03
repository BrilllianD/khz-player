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
