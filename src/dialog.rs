//! Native file and folder pickers (XDG portal through rfd), run off the UI thread.

use std::path::PathBuf;

use crossbeam_channel::Receiver;

use crate::library::tags::AUDIO_EXTENSIONS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    AddFiles,
    AddFolder,
}

#[derive(Debug)]
pub struct Picked {
    pub purpose: Purpose,
    pub paths: Vec<PathBuf>,
}

/// Extensions offered by the "Audio and playlists" filter.
pub fn extensions() -> Vec<&'static str> {
    AUDIO_EXTENSIONS
        .iter()
        .copied()
        .chain(["m3u", "m3u8"])
        .collect()
}

/// Opens the dialog on its own thread. The receiver yields one `Picked`; a
/// cancelled dialog gives an empty path list.
pub fn spawn(
    purpose: Purpose,
    start_dir: Option<PathBuf>,
    repaint: egui::Context,
) -> Receiver<Picked> {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let spawned = std::thread::Builder::new()
        .name("file-dialog".into())
        .spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title(match purpose {
                Purpose::AddFiles => "Add files",
                Purpose::AddFolder => "Add folder",
            });
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let result = match purpose {
                Purpose::AddFiles => dialog
                    .add_filter("Audio and playlists", &extensions())
                    .add_filter("All files", &["*"])
                    .pick_files(),
                Purpose::AddFolder => dialog.pick_folders(),
            };
            let _ = tx.send(Picked {
                purpose,
                paths: result.unwrap_or_default(),
            });
            repaint.request_repaint();
        });
    if let Err(e) = spawned {
        tracing::error!("spawn file dialog: {e}");
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_include_audio_and_playlists() {
        let exts = extensions();
        assert!(exts.contains(&"flac"));
        assert!(exts.contains(&"m3u8"));
        let mut sorted = exts.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), exts.len());
    }
}
