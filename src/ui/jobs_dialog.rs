//! "Downloads" modal: this session's download rows with Cancel, plus the yt-dlp/ffmpeg probe.

use egui::{Id, Modal, RichText};

use crate::app::{App, DownloadRow, DownloadState};
use crate::download::ToolStatus;

const PENDING: &str = "Checking yt-dlp and ffmpeg…";

pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.jobs_open {
        return;
    }
    let theme = app.theme.clone();
    let tools = app.tool_status().map(tool_lines);
    let rows: Vec<DownloadRow> = app.downloads().to_vec();

    let mut close = false;
    let mut cancel: Option<u64> = None;
    let resp = Modal::new(Id::new("jobs")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(RichText::new("Downloads").strong());
        tools_ui(ui, tools, &theme);
        ui.separator();
        if rows.is_empty() {
            ui.label(RichText::new("No downloads this session.").color(theme.text_dim));
        } else {
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    egui::Grid::new("jobs-grid").num_columns(3).show(ui, |ui| {
                        for r in &rows {
                            ui.add(egui::Label::new(row_label(r)).truncate())
                                .on_hover_text(&r.url);
                            let text = RichText::new(state_text(&r.state));
                            if matches!(r.state, DownloadState::Failed(_)) {
                                ui.label(text.color(theme.warn));
                            } else {
                                ui.label(text);
                            }
                            if cancellable(&r.state) {
                                if ui.small_button("Cancel").clicked() {
                                    cancel = Some(r.id);
                                }
                            } else {
                                ui.label("");
                            }
                            ui.end_row();
                        }
                    });
                });
        }
        if ui.button("Close").clicked() {
            close = true;
        }
    });
    if let Some(id) = cancel {
        app.cancel_download(id);
    }
    if close || resp.should_close() {
        app.jobs_open = false;
    }
}

/// The two tool lines as a grid, or the pending text while the probe runs.
pub(crate) fn tools_ui(
    ui: &mut egui::Ui,
    tools: Option<[(&'static str, String, bool); 2]>,
    theme: &crate::theme::Theme,
) {
    match tools {
        Some(lines) => {
            egui::Grid::new("tool-grid").num_columns(2).show(ui, |ui| {
                for (name, text, ok) in lines {
                    ui.label(name);
                    let color = if ok { theme.text_dim } else { theme.warn };
                    ui.label(RichText::new(text).color(color));
                    ui.end_row();
                }
            });
        }
        None => {
            ui.label(RichText::new(PENDING).color(theme.text_dim));
        }
    }
}

pub(crate) fn state_text(s: &DownloadState) -> String {
    match s {
        DownloadState::Queued => "queued".to_string(),
        DownloadState::Running { percent, stage } => {
            let mut parts: Vec<String> = Vec::new();
            if let Some(p) = percent {
                parts.push(format!("{p:.0}%"));
            }
            if let Some(st) = stage {
                parts.push(st.clone());
            }
            if parts.is_empty() {
                "running".to_string()
            } else {
                parts.join(" · ")
            }
        }
        DownloadState::Done => "done".to_string(),
        DownloadState::Failed(m) => format!("failed: {m}"),
        DownloadState::Cancelled => "cancelled".to_string(),
    }
}

pub(crate) fn cancellable(s: &DownloadState) -> bool {
    matches!(s, DownloadState::Queued | DownloadState::Running { .. })
}

pub(crate) fn row_label(r: &DownloadRow) -> &str {
    match &r.title {
        Some(t) if !t.trim().is_empty() => t,
        _ => &r.url,
    }
}

pub(crate) fn tool_lines(t: &ToolStatus) -> [(&'static str, String, bool); 2] {
    let line = |name: &'static str, r: &Result<String, String>| match r {
        Ok(v) => (name, v.clone(), true),
        Err(e) => (name, format!("unavailable: {e}"), false),
    };
    [line("yt-dlp", &t.ytdlp), line("ffmpeg", &t.ffmpeg)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(title: Option<&str>) -> DownloadRow {
        DownloadRow {
            id: 1,
            url: "https://x/y".into(),
            title: title.map(String::from),
            state: DownloadState::Queued,
        }
    }

    #[test]
    fn state_text_cases() {
        let run = |percent, stage: Option<&str>| DownloadState::Running {
            percent,
            stage: stage.map(String::from),
        };
        assert_eq!(state_text(&DownloadState::Queued), "queued");
        assert_eq!(state_text(&run(Some(41.6), Some("Merger"))), "42% · Merger");
        assert_eq!(state_text(&run(Some(41.6), None)), "42%");
        assert_eq!(state_text(&run(None, Some("Merger"))), "Merger");
        assert_eq!(state_text(&run(None, None)), "running");
        assert_eq!(state_text(&DownloadState::Done), "done");
        assert_eq!(
            state_text(&DownloadState::Failed("boom".into())),
            "failed: boom"
        );
        assert_eq!(state_text(&DownloadState::Cancelled), "cancelled");
    }

    #[test]
    fn cancellable_only_open_states() {
        assert!(cancellable(&DownloadState::Queued));
        assert!(cancellable(&DownloadState::Running {
            percent: None,
            stage: None
        }));
        assert!(!cancellable(&DownloadState::Done));
        assert!(!cancellable(&DownloadState::Failed("x".into())));
        assert!(!cancellable(&DownloadState::Cancelled));
    }

    #[test]
    fn row_label_prefers_title() {
        assert_eq!(row_label(&row(Some("Song"))), "Song");
        assert_eq!(row_label(&row(Some("  "))), "https://x/y");
        assert_eq!(row_label(&row(None)), "https://x/y");
    }

    #[test]
    fn tool_lines_ok_and_err() {
        let t = ToolStatus {
            ytdlp: Ok("2025.10.22".into()),
            ffmpeg: Err("not found".into()),
        };
        let l = tool_lines(&t);
        assert_eq!(l[0], ("yt-dlp", "2025.10.22".to_string(), true));
        assert_eq!(
            l[1],
            ("ffmpeg", "unavailable: not found".to_string(), false)
        );
    }
}
