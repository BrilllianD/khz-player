//! Download queue and wiring: one yt-dlp job runs at a time, the rest wait FIFO.

use std::collections::VecDeque;

use super::*;
use crate::download::{self, DownloadEvent, Request, ToolStatus, Tools};
use crate::job::{self, Terminal};

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadState {
    Queued,
    Running {
        percent: Option<f32>,
        stage: Option<String>,
    },
    Done,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DownloadRow {
    pub id: u64,
    pub url: String,
    pub title: Option<String>,
    pub state: DownloadState,
}

pub(super) struct Queued {
    pub(super) id: u64,
    pub(super) req: Request,
}

/// Rows of this session plus the FIFO of waiting requests. Pure: no threads.
#[derive(Default)]
pub(super) struct DownloadQueue {
    rows: Vec<DownloadRow>,
    waiting: VecDeque<Queued>,
    running: Option<u64>,
    next_id: u64,
}

impl DownloadQueue {
    pub(super) fn push(&mut self, req: Request) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.rows.push(DownloadRow {
            id,
            url: req.url.clone(),
            title: None,
            state: DownloadState::Queued,
        });
        self.waiting.push_back(Queued { id, req });
        id
    }

    /// Next waiting request; `None` while one runs. Marks its row running.
    pub(super) fn pop_next(&mut self) -> Option<Queued> {
        if self.running.is_some() {
            return None;
        }
        let q = self.waiting.pop_front()?;
        self.running = Some(q.id);
        self.set_state(
            q.id,
            DownloadState::Running {
                percent: None,
                stage: None,
            },
        );
        Some(q)
    }

    /// Event order across the two reader threads is free; only the terminal event is last.
    pub(super) fn apply(&mut self, id: u64, ev: &DownloadEvent) {
        let Some(row) = self.rows.iter_mut().find(|r| r.id == id) else {
            return;
        };
        let (mut percent, mut stage) = match &row.state {
            DownloadState::Running { percent, stage } => (*percent, stage.clone()),
            _ => (None, None),
        };
        match ev {
            DownloadEvent::Title(t) => row.title = Some(t.clone()),
            DownloadEvent::Progress { percent: p, .. } => percent = *p,
            DownloadEvent::Stage(s) => stage = Some(s.clone()),
            DownloadEvent::File(_) => {}
            DownloadEvent::Done { .. } => row.state = DownloadState::Done,
            DownloadEvent::Failed(m) => row.state = DownloadState::Failed(m.clone()),
            DownloadEvent::Cancelled => row.state = DownloadState::Cancelled,
        }
        if !ev.is_terminal() {
            row.state = DownloadState::Running { percent, stage };
        } else if self.running == Some(id) {
            self.running = None;
        }
    }

    pub(super) fn lost(&mut self, id: u64) {
        self.set_state(id, DownloadState::Failed("stopped unexpectedly".into()));
        if self.running == Some(id) {
            self.running = None;
        }
    }

    pub(super) fn cancel_waiting(&mut self, id: u64) -> bool {
        let Some(i) = self.waiting.iter().position(|q| q.id == id) else {
            return false;
        };
        self.waiting.remove(i);
        self.set_state(id, DownloadState::Cancelled);
        true
    }

    pub(super) fn rows(&self) -> &[DownloadRow] {
        &self.rows
    }

    pub(super) fn label(&self) -> Option<String> {
        let waiting = self.waiting.len();
        let running = self
            .running
            .and_then(|id| self.rows.iter().find(|r| r.id == id));
        if running.is_none() && waiting == 0 {
            return None;
        }
        if let Some(DownloadRow {
            state: DownloadState::Running {
                percent: Some(p), ..
            },
            ..
        }) = running
        {
            let mut s = format!("DL {:.0}%", p.clamp(0.0, 100.0));
            if waiting > 0 {
                s.push_str(&format!(" +{waiting}"));
            }
            return Some(s);
        }
        Some(format!("DL {}", usize::from(running.is_some()) + waiting))
    }

    fn set_state(&mut self, id: u64, state: DownloadState) {
        if let Some(r) = self.rows.iter_mut().find(|r| r.id == id) {
            r.state = state;
        }
    }
}

/// Download folder: configured one, else `<first root>/Downloads`, else `<audio dir>/Downloads`.
pub(super) fn destination(
    folder: Option<&Path>,
    roots: &[PathBuf],
    audio_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(f) = folder {
        return Some(f.to_path_buf());
    }
    if let Some(r) = roots.first() {
        return Some(r.join("Downloads"));
    }
    audio_dir.map(|d| d.join("Downloads"))
}

pub(super) fn inside_root(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

/// Result of the one-shot tool probe.
pub(super) struct Probed(pub(super) ToolStatus);

impl Terminal for Probed {
    fn is_terminal(&self) -> bool {
        true
    }
}

pub(super) struct Running {
    id: u64,
    job: Job<DownloadEvent>,
    warned_outside: bool,
}

#[derive(Default)]
pub(super) struct Downloads {
    queue: DownloadQueue,
    running: Option<Running>,
    /// Library-root files waiting for the scan to end before they are upserted.
    pending: Vec<Track>,
    probe: Option<Job<Probed>>,
    tools: Option<ToolStatus>,
}

impl App {
    pub fn start_download(&mut self, url: &str, keep_video: bool) {
        let url = match download::validate_url(url) {
            Ok(u) => u,
            Err(e) => {
                self.toast(format!("Download: {e}"));
                return;
            }
        };
        let folder = self.cfg.download.folder.as_deref().map(expand_tilde);
        let roots = self.expanded_roots();
        let audio = directories::UserDirs::new().and_then(|u| u.audio_dir().map(Path::to_path_buf));
        let Some(dest) = destination(folder.as_deref(), &roots, audio) else {
            self.toast("Download: no destination folder (set download.folder in config.toml)");
            return;
        };
        let req = Request {
            url,
            dest,
            keep_video,
            playlist: self.cfg.download.playlist,
            max_height: self.cfg.download.max_height,
        };
        self.dl.queue.push(req);
        self.start_next_download();
    }

    pub fn cancel_download(&mut self, id: u64) {
        match &self.dl.running {
            Some(r) if r.id == id => r.job.cancel(),
            _ => {
                self.dl.queue.cancel_waiting(id);
            }
        }
    }

    pub fn downloads(&self) -> &[DownloadRow] {
        self.dl.queue.rows()
    }

    pub fn download_label(&self) -> Option<String> {
        self.dl.queue.label()
    }

    /// Cached tool probe; the first call starts it on a job and returns `None`.
    pub fn tool_status(&mut self) -> Option<&ToolStatus> {
        if self.dl.tools.is_none() && self.dl.probe.is_none() {
            self.dl.probe = Some(job::spawn("tool-probe", self.ctx.clone(), |sink, _| {
                sink.send(Probed(download::probe_tools(&Tools::default())));
            }));
        }
        self.dl.tools.as_ref()
    }

    pub fn modal_open(&self) -> bool {
        self.prompt.is_some()
            || self.jump.is_some()
            || self.info.is_some()
            || self.settings_open
            || self.jobs_open
    }

    /// Best effort on exit: the job thread sends the SIGTERM.
    pub(super) fn cancel_running_download(&self) {
        if let Some(r) = &self.dl.running {
            r.job.cancel();
        }
    }

    fn expanded_roots(&self) -> Vec<PathBuf> {
        self.cfg
            .library_roots
            .iter()
            .map(|r| expand_tilde(r))
            .collect()
    }

    fn start_next_download(&mut self) {
        if self.dl.running.is_some() {
            return;
        }
        if let Some(q) = self.dl.queue.pop_next() {
            let job = download::start(
                q.req,
                Tools::default(),
                download::temp_root(),
                self.ctx.clone(),
            );
            self.dl.running = Some(Running {
                id: q.id,
                job,
                warned_outside: false,
            });
        }
    }

    pub(super) fn handle_download_events(&mut self) {
        if let Some(run) = &mut self.dl.running {
            let polled = run.job.poll();
            let finished = run.job.is_finished();
            let id = run.id;
            for item in polled {
                match item {
                    Poll::Event(DownloadEvent::File(p)) => self.download_file(p),
                    Poll::Event(ev) => {
                        if let DownloadEvent::Failed(m) = &ev {
                            self.toast(format!("Download failed: {m}"));
                        }
                        self.dl.queue.apply(id, &ev);
                    }
                    Poll::Lost => {
                        self.dl.queue.lost(id);
                        self.toast("Download stopped unexpectedly");
                    }
                }
            }
            if finished {
                self.dl.running = None;
                self.start_next_download();
            }
        }
        if let Some(probe) = &mut self.dl.probe {
            let polled = probe.poll();
            let finished = probe.is_finished();
            for item in polled {
                self.dl.tools = Some(match item {
                    Poll::Event(Probed(s)) => s,
                    Poll::Lost => ToolStatus {
                        ytdlp: Err("probe stopped".into()),
                        ffmpeg: Err("probe stopped".into()),
                    },
                });
            }
            if finished {
                self.dl.probe = None;
            }
        }
        self.flush_download_pending();
    }

    fn download_file(&mut self, path: PathBuf) {
        let t = tags::read(&path);
        self.playlists[self.active].add([t.clone()]);
        self.after_playlist_edit();
        let roots = self.expanded_roots();
        if inside_root(&path, &roots) {
            if self.db.is_some() {
                self.dl.pending.push(t);
            } else {
                tracing::warn!(
                    "no library database: {} added to the playlist only",
                    path.display()
                );
            }
        } else if let Some(r) = &mut self.dl.running
            && !r.warned_outside
        {
            r.warned_outside = true;
            self.toast("Downloaded outside the library folders: added to the playlist only");
        }
    }

    fn flush_download_pending(&mut self) {
        if self.scan.is_some() || self.dl.pending.is_empty() {
            return;
        }
        let pending = std::mem::take(&mut self.dl.pending);
        let Some(db) = &self.db else { return };
        let result = pending
            .iter()
            .try_for_each(|t| db::upsert_one(db, t))
            .and_then(|()| db::load_tracks(db));
        match result {
            Ok(tracks) => {
                self.library = tracks;
                self.library_gen += 1;
            }
            Err(e) => self.toast(format!("Library update failed: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(url: &str) -> Request {
        Request {
            url: url.into(),
            dest: PathBuf::from("/d"),
            keep_video: false,
            playlist: false,
            max_height: 1080,
        }
    }

    fn done() -> DownloadEvent {
        DownloadEvent::Done { files: vec![] }
    }

    fn prog(p: f32) -> DownloadEvent {
        DownloadEvent::Progress {
            percent: Some(p),
            eta_secs: None,
        }
    }

    #[test]
    fn queue_runs_one_at_a_time_fifo() {
        let mut q = DownloadQueue::default();
        assert_eq!(q.push(req("a")), 1);
        q.push(req("b"));
        q.push(req("c"));
        assert_eq!(q.pop_next().map(|x| x.id), Some(1));
        assert!(q.pop_next().is_none());
        q.apply(1, &done());
        assert_eq!(q.pop_next().map(|x| x.id), Some(2));
        assert!(q.pop_next().is_none());
    }

    #[test]
    fn events_in_any_order() {
        let mut q = DownloadQueue::default();
        let id = q.push(req("a"));
        q.pop_next();
        q.apply(id, &prog(10.0));
        q.apply(id, &DownloadEvent::File(PathBuf::from("/d/x.opus")));
        q.apply(id, &DownloadEvent::Stage("ExtractAudio".into()));
        q.apply(id, &DownloadEvent::Title("T".into()));
        q.apply(id, &prog(55.0));
        let row = &q.rows()[0];
        assert_eq!(row.title.as_deref(), Some("T"));
        assert_eq!(
            row.state,
            DownloadState::Running {
                percent: Some(55.0),
                stage: Some("ExtractAudio".into())
            }
        );
        let id2 = q.push(req("b"));
        q.apply(id, &done());
        q.pop_next();
        q.apply(id2, &prog(1.0));
        assert_eq!(q.rows()[1].title, None);
    }

    #[test]
    fn terminal_states() {
        let mut q = DownloadQueue::default();
        for u in ["a", "b", "c"] {
            q.push(req(u));
        }
        q.pop_next();
        q.apply(1, &DownloadEvent::Failed("boom".into()));
        assert_eq!(q.rows()[0].state, DownloadState::Failed("boom".into()));
        q.pop_next();
        q.apply(2, &DownloadEvent::Cancelled);
        assert_eq!(q.rows()[1].state, DownloadState::Cancelled);
        q.pop_next();
        q.lost(3);
        assert_eq!(
            q.rows()[2].state,
            DownloadState::Failed("stopped unexpectedly".into())
        );
        assert_eq!(q.label(), None);
    }

    #[test]
    fn cancel_waiting_skips_request() {
        let mut q = DownloadQueue::default();
        q.push(req("a"));
        q.push(req("b"));
        q.push(req("c"));
        q.pop_next();
        assert!(q.cancel_waiting(2));
        assert_eq!(q.rows()[1].state, DownloadState::Cancelled);
        assert!(!q.cancel_waiting(1), "running id");
        assert!(!q.cancel_waiting(99), "unknown id");
        q.apply(1, &done());
        assert_eq!(q.pop_next().map(|x| x.id), Some(3));
    }

    #[test]
    fn label_rules() {
        let mut q = DownloadQueue::default();
        assert_eq!(q.label(), None);
        q.push(req("a"));
        assert_eq!(q.label().as_deref(), Some("DL 1"));
        q.pop_next();
        assert_eq!(q.label().as_deref(), Some("DL 1"));
        q.apply(1, &prog(42.4));
        assert_eq!(q.label().as_deref(), Some("DL 42%"));
        q.push(req("b"));
        q.push(req("c"));
        assert_eq!(q.label().as_deref(), Some("DL 42% +2"));
        q.apply(1, &done());
        assert_eq!(q.label().as_deref(), Some("DL 2"));
        q.pop_next();
        assert_eq!(q.label().as_deref(), Some("DL 2"));
        q.push(req("d"));
        assert_eq!(q.label().as_deref(), Some("DL 3"));
    }

    #[test]
    fn destination_rule() {
        let roots = vec![PathBuf::from("/m/a"), PathBuf::from("/m/b")];
        let audio = Some(PathBuf::from("/home/u/Music"));
        assert_eq!(
            destination(Some(Path::new("/f")), &roots, audio.clone()),
            Some(PathBuf::from("/f"))
        );
        assert_eq!(
            destination(None, &roots, audio.clone()),
            Some(PathBuf::from("/m/a/Downloads"))
        );
        assert_eq!(
            destination(None, &[], audio),
            Some(PathBuf::from("/home/u/Music/Downloads"))
        );
        assert_eq!(destination(None, &[], None), None);
    }

    #[test]
    fn inside_root_rule() {
        let roots = vec![PathBuf::from("/m/a")];
        assert!(inside_root(Path::new("/m/a/x.opus"), &roots));
        assert!(!inside_root(Path::new("/m/ab/x.opus"), &roots));
    }
}
