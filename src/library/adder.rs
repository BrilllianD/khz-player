//! Background add: expands files, folders and M3U lists into tracks off the UI thread.

use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;
use rusqlite::Connection;
use walkdir::WalkDir;

use super::{Track, db, tags};
use crate::m3u;

/// Tracks per `AddEvent::Tracks`.
pub const BATCH: usize = 100;

#[derive(Debug)]
pub enum AddEvent {
    Tracks(Vec<Track>),
    Problem(String),
    Done,
}

pub fn is_playlist_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("m3u") || e.eq_ignore_ascii_case("m3u8"))
}

/// Track metadata from the library, else read from the file.
fn track_for(conn: Option<&Connection>, path: &Path) -> Track {
    conn.and_then(|c| db::get_track(c, path).ok().flatten())
        .unwrap_or_else(|| tags::read(path))
}

fn send_chunks(tracks: Vec<Track>, send: &dyn Fn(AddEvent)) {
    let mut it = tracks.into_iter().peekable();
    while it.peek().is_some() {
        send(AddEvent::Tracks(it.by_ref().take(BATCH).collect()));
    }
}

fn collect(paths: &[PathBuf], conn: Option<&Connection>, send: &dyn Fn(AddEvent)) {
    for p in paths {
        if p.is_dir() {
            let mut files: Vec<PathBuf> = WalkDir::new(p)
                .follow_links(true)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file() && tags::is_audio(e.path()))
                .map(|e| e.into_path())
                .collect();
            files.sort();
            let mut batch = Vec::new();
            for f in &files {
                batch.push(track_for(conn, f));
                if batch.len() == BATCH {
                    send(AddEvent::Tracks(std::mem::take(&mut batch)));
                }
            }
            send_chunks(batch, send);
        } else if is_playlist_file(p) {
            match m3u::read(p) {
                Ok(entries) => {
                    let tracks = entries
                        .iter()
                        .map(|e| {
                            if e.path.exists() {
                                track_for(conn, &e.path)
                            } else {
                                let mut t = Track::from_path(e.path.clone());
                                t.title = e.title.clone();
                                t.duration_ms = e.duration_secs.map(|s| s as u64 * 1000);
                                t
                            }
                        })
                        .collect();
                    send_chunks(tracks, send);
                }
                Err(e) => send(AddEvent::Problem(format!(
                    "Cannot read {}: {e}",
                    p.display()
                ))),
            }
        } else if p.is_file() {
            send(AddEvent::Tracks(vec![track_for(conn, p)]));
        } else {
            send(AddEvent::Problem(format!("Not found: {}", p.display())));
        }
    }
}

pub fn start(paths: Vec<PathBuf>, db_path: PathBuf, repaint: egui::Context) -> Receiver<AddEvent> {
    let (tx, rx) = crossbeam_channel::unbounded();
    let spawned = std::thread::Builder::new()
        .name("playlist-add".into())
        .spawn(move || {
            let send = |ev: AddEvent| {
                let _ = tx.send(ev);
                repaint.request_repaint();
            };
            let conn = db::open(&db_path).ok();
            collect(&paths, conn.as_ref(), &send);
            send(AddEvent::Done);
        });
    if let Err(e) = spawned {
        tracing::error!("spawn playlist adder: {e}");
    }
    rx
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::config::test_dir;
    use crate::library::scanner::tests::write_wav;

    fn run(paths: &[PathBuf]) -> Vec<AddEvent> {
        let sink = RefCell::new(Vec::new());
        collect(paths, None, &|ev| sink.borrow_mut().push(ev));
        sink.into_inner()
    }

    fn track_paths(events: &[AddEvent]) -> Vec<PathBuf> {
        events
            .iter()
            .flat_map(|e| match e {
                AddEvent::Tracks(t) => t.iter().map(|t| t.path.clone()).collect(),
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn folder_is_sorted_and_batched() {
        let d = test_dir("adder-folder");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        write_wav(&d.join("b.wav"), 800);
        write_wav(&d.join("a.wav"), 800);
        write_wav(&d.join("sub/c.wav"), 800);
        std::fs::write(d.join("notes.txt"), "x").unwrap();
        let ev = run(std::slice::from_ref(&d));
        assert_eq!(
            track_paths(&ev),
            vec![d.join("a.wav"), d.join("b.wav"), d.join("sub/c.wav")]
        );
        assert!(ev.iter().all(|e| match e {
            AddEvent::Tracks(t) => !t.is_empty() && t.len() <= BATCH,
            _ => true,
        }));
    }

    #[test]
    fn m3u_keeps_missing_entries_with_title() {
        let d = test_dir("adder-m3u");
        write_wav(&d.join("here.wav"), 800);
        let list = d.join("l.m3u");
        std::fs::write(
            &list,
            format!(
                "#EXTM3U\n{}\n#EXTINF:12,Gone\n{}\n",
                d.join("here.wav").display(),
                d.join("gone.mp3").display()
            ),
        )
        .unwrap();
        let ev = run(&[list]);
        let tracks: Vec<&Track> = ev
            .iter()
            .flat_map(|e| match e {
                AddEvent::Tracks(t) => t.iter().collect(),
                _ => Vec::new(),
            })
            .collect();
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].title.as_deref(), Some("Gone"));
        assert_eq!(tracks[1].duration_ms, Some(12000));
    }

    #[test]
    fn missing_path_reports_not_found() {
        let d = test_dir("adder-missing");
        let ev = run(&[d.join("nope.wav")]);
        assert_eq!(ev.len(), 1);
        assert!(matches!(&ev[0], AddEvent::Problem(m) if m.starts_with("Not found: ")));
    }

    #[test]
    fn paths_keep_request_order() {
        let d = test_dir("adder-order");
        std::fs::create_dir_all(d.join("dir")).unwrap();
        write_wav(&d.join("dir/a.wav"), 800);
        write_wav(&d.join("z.wav"), 800);
        let ev = run(&[d.join("z.wav"), d.join("dir")]);
        assert_eq!(track_paths(&ev), vec![d.join("z.wav"), d.join("dir/a.wav")]);
    }
}
