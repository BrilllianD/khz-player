//! yt-dlp download job. Verified 2026-11-27 with yt-dlp 2026.10.26 (notes in ~/mem khz-player
//! `notes/yt-dlp-output-contract.md`): `KHZ|T`, `KHZ|P`, `KHZ|F` lines arrive on stdout,
//! `KHZ|S` and `WARNING:`/`ERROR:` on stderr (both pipes are read). Missing numbers render as `NA`.
//! R1 progress shows without `--no-quiet`: no flag added. R2 `NA` is the only token: nothing
//! added. R3 `--embed-thumbnail` works for Opus with mutagen installed (cover in the `.opus`).
//! R4 a repeated download still prints `KHZ|F`: no `--force-overwrites`. Audio `.opus`, AAC
//! only `.m4a`, video `.mp4`. Postprocessors: ExtractAudio, FixupM4a, Merger, Metadata,
//! EmbedThumbnail, MoveFiles.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::RecvTimeoutError;

use crate::job::{self, Cancel, Job, Sink, Terminal};

/// Grace between SIGTERM and SIGKILL on cancel.
const KILL_AFTER: Duration = Duration::from_secs(3);

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Paths of the external tools; the default finds both on `PATH`.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ytdlp: PathBuf,
    pub ffmpeg: PathBuf,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            ytdlp: PathBuf::from("yt-dlp"),
            ffmpeg: PathBuf::from("ffmpeg"),
        }
    }
}

/// Result of probing the tools: `Ok(version line)` or the reason it failed.
#[derive(Debug, Clone)]
pub struct ToolStatus {
    pub ytdlp: Result<String, String>,
    pub ffmpeg: Result<String, String>,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub url: String,
    pub dest: PathBuf,
    pub keep_video: bool,
    pub playlist: bool,
    pub max_height: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DownloadEvent {
    Title(String),
    Progress {
        percent: Option<f32>,
        eta_secs: Option<u64>,
    },
    /// Postprocessor name.
    Stage(String),
    /// Final path in `dest`, once per finished item.
    File(PathBuf),
    Done {
        files: Vec<PathBuf>,
    },
    Failed(String),
    Cancelled,
}

impl Terminal for DownloadEvent {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            DownloadEvent::Done { .. } | DownloadEvent::Failed(_) | DownloadEvent::Cancelled
        )
    }
}

/// Runs `<ytdlp> --version` and `<ffmpeg> -version`. Blocking; never call on the UI thread.
pub fn probe_tools(tools: &Tools) -> ToolStatus {
    ToolStatus {
        ytdlp: probe_one(&tools.ytdlp, "--version"),
        ffmpeg: probe_one(&tools.ffmpeg, "-version"),
    }
}

fn probe_one(bin: &Path, flag: &str) -> Result<String, String> {
    let out = Command::new(bin)
        .arg(flag)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text.lines().next().unwrap_or("").trim().to_string())
    } else {
        let text = String::from_utf8_lossy(&out.stderr);
        let first = text.lines().next().unwrap_or("").trim();
        Err(format!("exit {}: {first}", out.status))
    }
}

/// Trimmed URL, or why it cannot be passed to yt-dlp.
pub fn validate_url(s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("empty URL".into());
    }
    if t.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("URL must be one line".into());
    }
    let lower = t.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"));
    match rest {
        Some(r) if !r.is_empty() => Ok(t.to_string()),
        _ => Err("URL must start with http:// or https://".into()),
    }
}

/// Parent of the per-job temp dirs.
pub fn temp_root() -> PathBuf {
    crate::config::project_dirs()
        .map(|d| d.cache_dir().join("downloads"))
        .unwrap_or_else(|| std::env::temp_dir().join("khz-player-downloads"))
}

fn na_none(s: &str) -> Option<f64> {
    match s.trim() {
        "" | "NA" | "None" => None,
        t => t.parse::<f64>().ok().filter(|v| v.is_finite()),
    }
}

/// Parses one `KHZ|` protocol line; anything else is `None`.
pub fn parse_line(line: &str) -> Option<DownloadEvent> {
    let line = line.trim_end_matches(['\r', '\n']);
    let rest = line.strip_prefix("KHZ|")?;
    let (tag, body) = rest.split_once('|')?;
    match tag {
        "T" => Some(DownloadEvent::Title(body.to_string())),
        "F" => Some(DownloadEvent::File(PathBuf::from(body))),
        "S" => {
            let (name, status) = body.split_once('|')?;
            (status == "started").then(|| DownloadEvent::Stage(name.to_string()))
        }
        "P" => {
            let f: Vec<&str> = body.splitn(4, '|').collect();
            if f.len() != 4 {
                return None;
            }
            let downloaded = na_none(f[0]);
            let total = na_none(f[1]).filter(|v| *v > 0.0);
            let estimate = na_none(f[2]).filter(|v| *v > 0.0);
            let percent = match (downloaded, total.or(estimate)) {
                (Some(d), Some(t)) => Some((d / t * 100.0).clamp(0.0, 100.0) as f32),
                _ => None,
            };
            let eta_secs = na_none(f[3])
                .filter(|v| *v >= 0.0)
                .map(|v| v.round() as u64);
            Some(DownloadEvent::Progress { percent, eta_secs })
        }
        _ => None,
    }
}

fn prefixed(prefix: &str, path: &Path) -> OsString {
    let mut s = OsString::from(prefix);
    s.push(path.as_os_str());
    s
}

/// The yt-dlp argv for `req`; the user's yt-dlp config still applies (no `--ignore-config`).
pub fn build_args(req: &Request, job_temp: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    a.extend([
        "--newline",
        "--progress",
        "--no-simulate",
        "--color",
        "never",
        "--no-exec",
        "--progress-template",
        "download:KHZ|P|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.eta)s",
        "--progress-template",
        "postprocess:KHZ|S|%(progress.postprocessor)s|%(progress.status)s",
        "--print",
        "before_dl:KHZ|T|%(title)s",
        "--print",
        "after_move:KHZ|F|%(filepath)s",
        "-o",
        "%(title).180B [%(id)s].%(ext)s",
    ].map(OsString::from));
    a.push("-P".into());
    a.push(prefixed("home:", &req.dest));
    a.push("-P".into());
    a.push(prefixed("temp:", job_temp));
    let mut tail: Vec<String> = vec![
        if req.playlist {
            "--yes-playlist"
        } else {
            "--no-playlist"
        }
        .into(),
        "--embed-metadata".into(),
        "--embed-thumbnail".into(),
    ];
    if req.keep_video {
        let h = req.max_height;
        tail.extend([
            "-f".into(),
            format!("bv*[height<=?{h}]+ba/b[height<=?{h}]"),
            "-S".into(),
            "vcodec:h264,acodec:aac".into(),
            "--merge-output-format".into(),
            "mp4".into(),
        ]);
    } else {
        tail.extend(["-f".into(), "bestaudio/best".into(), "-x".into()]);
    }
    tail.push("--".into());
    a.extend(tail.into_iter().map(OsString::from));
    a.push(OsString::from(&req.url));
    a
}

fn kill_group(pid: u32, sig: libc::c_int) {
    // SAFETY: plain syscall with a negative pid (process group); no memory is touched. The
    // result is ignored: ESRCH means the group is already gone.
    unsafe {
        libc::kill(-(pid as libc::pid_t), sig);
    }
}

/// Reads `r` line by line (lossy UTF-8) into `tx` until EOF.
fn pump<R: Read + Send + 'static>(r: R, tx: crossbeam_channel::Sender<String>) {
    std::thread::spawn(move || {
        let mut br = BufReader::new(r);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match br.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

fn spawn_child(tools: &Tools, args: &[OsString]) -> std::io::Result<std::process::Child> {
    let mut tries = 0;
    loop {
        let r = Command::new(&tools.ytdlp)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PYTHONUNBUFFERED", "1")
            .process_group(0)
            .spawn();
        match r {
            // A script written a moment ago can still be open for write in another thread's fork.
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && tries < 5 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

/// Starts the download on a background job. Exactly one terminal event ends it.
pub fn start(
    req: Request,
    tools: Tools,
    temp_root: PathBuf,
    repaint: egui::Context,
) -> Job<DownloadEvent> {
    job::spawn("download", repaint, move |sink, cancel| {
        let n = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
        let job_dir = temp_root.join(format!("job-{}-{}", std::process::id(), n));
        let ev = run(&req, &tools, &job_dir, sink, cancel);
        let _ = std::fs::remove_dir_all(&job_dir);
        sink.send(ev);
    })
}

/// The job body; returns the terminal event. The caller removes `job_dir`.
fn run(
    req: &Request,
    tools: &Tools,
    job_dir: &Path,
    sink: &Sink<DownloadEvent>,
    cancel: &Cancel,
) -> DownloadEvent {
    if let Err(e) = std::fs::create_dir_all(job_dir) {
        return DownloadEvent::Failed(format!("cannot create {}: {e}", job_dir.display()));
    }
    if let Err(e) = std::fs::create_dir_all(&req.dest) {
        return DownloadEvent::Failed(format!("cannot create {}: {e}", req.dest.display()));
    }
    let mut child = match spawn_child(tools, &build_args(req, job_dir)) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return DownloadEvent::Failed(format!("yt-dlp not found: {}", tools.ytdlp.display()));
        }
        Err(e) => return DownloadEvent::Failed(format!("cannot run yt-dlp: {e}")),
    };
    let pid = child.id();
    let (tx, rx) = crossbeam_channel::unbounded::<String>();
    if let Some(o) = child.stdout.take() {
        pump(o, tx.clone());
    }
    if let Some(e) = child.stderr.take() {
        pump(e, tx.clone());
    }
    drop(tx);

    let mut files: Vec<PathBuf> = Vec::new();
    let mut last_error: Option<String> = None;
    loop {
        if cancel.is_set() {
            kill_group(pid, libc::SIGTERM);
            let deadline = Instant::now() + KILL_AFTER;
            while Instant::now() < deadline {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            // Also sweeps stragglers (an ffmpeg child) once the leader is gone.
            kill_group(pid, libc::SIGKILL);
            let _ = child.wait();
            return DownloadEvent::Cancelled;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(raw) => {
                let line = raw.trim_end_matches(['\r', '\n']);
                if let Some(ev) = parse_line(line) {
                    if let DownloadEvent::File(p) = &ev {
                        files.push(p.clone());
                    }
                    sink.send(ev);
                } else if let Some(msg) = line.trim().strip_prefix("ERROR:") {
                    last_error = Some(msg.trim().to_string());
                } else {
                    tracing::debug!("yt-dlp: {line}");
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    match child.wait() {
        Ok(st) if st.success() => DownloadEvent::Done { files },
        Ok(st) => {
            DownloadEvent::Failed(last_error.unwrap_or_else(|| format!("yt-dlp exited with {st}")))
        }
        Err(e) => DownloadEvent::Failed(format!("cannot wait for yt-dlp: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Poll;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    fn tdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("khz-dl-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        write!(f, "#!/bin/sh\n{body}\n").unwrap();
        drop(f);
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    const FIND: &str = r#"for a in "$@"; do case "$a" in home:*) H="${a#home:}";; temp:*) T="${a#temp:}";; esac; done"#;

    fn req(dest: &Path) -> Request {
        Request {
            url: "https://example.com/v".into(),
            dest: dest.to_path_buf(),
            keep_video: false,
            playlist: false,
            max_height: 1080,
        }
    }

    fn tools(ytdlp: PathBuf) -> Tools {
        Tools {
            ytdlp,
            ffmpeg: PathBuf::from("ffmpeg"),
        }
    }

    fn drain(job: &mut Job<DownloadEvent>) -> Vec<DownloadEvent> {
        let start = Instant::now();
        let mut out = Vec::new();
        while !job.is_finished() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "job did not finish"
            );
            for p in job.poll() {
                match p {
                    Poll::Event(e) => out.push(e),
                    Poll::Lost => panic!("job lost"),
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        out
    }

    fn run_fake(name: &str, body: &str) -> (Vec<DownloadEvent>, PathBuf, PathBuf) {
        let base = tdir(name);
        let script = fake(&base, "yt-dlp", body);
        let root = base.join("root");
        let dest = base.join("dest");
        let mut job = start(
            req(&dest),
            tools(script),
            root.clone(),
            egui::Context::default(),
        );
        (drain(&mut job), base, root)
    }

    fn gone(pid: i32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
                Err(_) => return true,
                Ok(s) if s.rsplit(") ").next().is_some_and(|r| r.starts_with('Z')) => return true,
                Ok(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        false
    }

    #[test]
    fn validate_url_accepts_and_trims() {
        assert_eq!(
            validate_url("  https://a.b/c?d=1 \n").unwrap(),
            "https://a.b/c?d=1"
        );
        assert_eq!(validate_url("HTTP://a.b").unwrap(), "HTTP://a.b");
    }

    #[test]
    fn validate_url_rejects() {
        assert_eq!(validate_url("  ").unwrap_err(), "empty URL");
        assert_eq!(
            validate_url("https://a\nhttps://b").unwrap_err(),
            "URL must be one line"
        );
        assert_eq!(
            validate_url("https://a b").unwrap_err(),
            "URL must be one line"
        );
        let scheme = "URL must start with http:// or https://";
        assert_eq!(validate_url("-o /x").unwrap_err(), "URL must be one line");
        assert_eq!(validate_url("--version").unwrap_err(), scheme);
        assert_eq!(validate_url("ftp://a").unwrap_err(), scheme);
        assert_eq!(validate_url("https://").unwrap_err(), scheme);
    }

    #[test]
    fn parse_line_title_and_file() {
        assert_eq!(
            parse_line("KHZ|T|Me at the zoo"),
            Some(DownloadEvent::Title("Me at the zoo".into()))
        );
        assert_eq!(
            parse_line("KHZ|T|a|b|c\r\n"),
            Some(DownloadEvent::Title("a|b|c".into()))
        );
        assert_eq!(
            parse_line("KHZ|F|/tmp/khz-dl-run/a/home/Me at the zoo [jNQXAC9IVRw].opus"),
            Some(DownloadEvent::File(PathBuf::from(
                "/tmp/khz-dl-run/a/home/Me at the zoo [jNQXAC9IVRw].opus"
            )))
        );
    }

    #[test]
    fn parse_line_progress() {
        let p = |percent, eta_secs| Some(DownloadEvent::Progress { percent, eta_secs });
        assert_eq!(
            parse_line("KHZ|P|1024|252182|NA|NA"),
            p(Some(1024.0 / 252182.0 * 100.0), None)
        );
        assert_eq!(
            parse_line("KHZ|P|3072|252182|NA|0"),
            p(Some(3072.0 / 252182.0 * 100.0), Some(0))
        );
        assert_eq!(parse_line("KHZ|P|50|NA|100|3.4"), p(Some(50.0), Some(3)));
        assert_eq!(parse_line("KHZ|P|50|NA|NA|3"), p(None, Some(3)));
        assert_eq!(parse_line("KHZ|P|150|100|NA|None"), p(Some(100.0), None));
        assert_eq!(parse_line("KHZ|P|NA|NA|NA|NA"), p(None, None));
        assert_eq!(parse_line("KHZ|P|5|0|NA|"), p(None, None));
        assert_eq!(parse_line("KHZ|P|1|2|3"), None);
    }

    #[test]
    fn parse_line_stage_and_noise() {
        assert_eq!(
            parse_line("KHZ|S|ExtractAudio|started"),
            Some(DownloadEvent::Stage("ExtractAudio".into()))
        );
        assert_eq!(parse_line("KHZ|S|ExtractAudio|finished"), None);
        assert_eq!(parse_line("KHZ|X|1"), None);
        assert_eq!(parse_line("KHZ|S|nostatus"), None);
        assert_eq!(parse_line("WARNING: KHZ|T|x"), None);
        assert_eq!(parse_line(""), None);
    }

    fn strs(a: &[OsString]) -> Vec<String> {
        a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn build_args_audio() {
        let r = req(Path::new("/d/out"));
        let a = strs(&build_args(&r, Path::new("/t/job-1")));
        assert_eq!(&a[a.len() - 2..], ["--", "https://example.com/v"]);
        assert!(a.windows(2).any(|w| w == ["-P", "temp:/t/job-1"]));
        assert!(a.windows(2).any(|w| w == ["-P", "home:/d/out"]));
        assert!(a.windows(3).any(|w| w == ["-f", "bestaudio/best", "-x"]));
        for f in [
            "--no-playlist",
            "--embed-metadata",
            "--embed-thumbnail",
            "--no-simulate",
            "--no-exec",
        ] {
            assert!(a.iter().any(|x| x == f), "{f}");
        }
        for f in [
            "--ignore-config",
            "--no-config",
            "--audio-format",
            "--yes-playlist",
            "--merge-output-format",
        ] {
            assert!(!a.iter().any(|x| x == f), "{f}");
        }
        assert_eq!(a.iter().filter(|x| *x == "--progress-template").count(), 2);
    }

    #[test]
    fn build_args_video() {
        let mut r = req(Path::new("/d"));
        r.keep_video = true;
        r.playlist = true;
        r.max_height = 720;
        let a = strs(&build_args(&r, Path::new("/t/j")));
        assert!(
            a.windows(2)
                .any(|w| w == ["-f", "bv*[height<=?720]+ba/b[height<=?720]"])
        );
        assert!(a.windows(2).any(|w| w == ["-S", "vcodec:h264,acodec:aac"]));
        assert!(a.windows(2).any(|w| w == ["--merge-output-format", "mp4"]));
        assert!(a.iter().any(|x| x == "--yes-playlist"));
        assert!(a.iter().any(|x| x == "--embed-thumbnail"));
        assert!(
            !a.iter()
                .any(|x| x == "-x" || x == "--no-playlist" || x == "--ignore-config")
        );
        assert_eq!(a.last().unwrap(), "https://example.com/v");
    }

    #[test]
    fn probe_tools_reports_both() {
        let base = tdir("probe");
        let ok = fake(&base, "yt-dlp", "echo 2026.10.26");
        let st = probe_tools(&Tools {
            ytdlp: ok,
            ffmpeg: PathBuf::from("/nonexistent/ffmpeg"),
        });
        assert_eq!(st.ytdlp, Ok("2026.10.26".into()));
        assert!(st.ffmpeg.is_err());
        let bad = fake(&base, "bad", "echo oops >&2; exit 2");
        let st = probe_tools(&tools(bad));
        let e = st.ytdlp.unwrap_err();
        assert!(e.starts_with("exit ") && e.ends_with("oops"), "{e}");
    }

    #[test]
    fn fake_success() {
        let base = tdir("success");
        let body = format!(
            r#"{FIND}
printf '%s\n' "$@" > {b}/argv
echo 'KHZ|T|Song'
echo 'KHZ|P|50|100|NA|3' >&2
echo 'KHZ|S|ExtractAudio|started'
test -d "$T" || exit 4
: > "$H/a.opus"
echo "KHZ|F|$H/a.opus"
echo garbage
exit 0"#,
            b = base.display()
        );
        let script = fake(&base, "yt-dlp", &body);
        let root = base.join("root");
        let dest = base.join("dest");
        let mut job = start(
            req(&dest),
            tools(script),
            root.clone(),
            egui::Context::default(),
        );
        let evs = drain(&mut job);
        let file = dest.join("a.opus");
        assert_eq!(evs.first(), Some(&DownloadEvent::Title("Song".into())));
        assert!(evs.contains(&DownloadEvent::Progress {
            percent: Some(50.0),
            eta_secs: Some(3)
        }));
        assert!(evs.contains(&DownloadEvent::Stage("ExtractAudio".into())));
        assert!(evs.contains(&DownloadEvent::File(file.clone())));
        assert_eq!(evs.last(), Some(&DownloadEvent::Done { files: vec![file] }));
        let argv: Vec<String> = std::fs::read_to_string(base.join("argv"))
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(&argv[argv.len() - 2..], ["--", "https://example.com/v"]);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn fake_failure() {
        let (evs, _b, root) = run_fake("failure", "echo 'ERROR: [x] boom' >&2; exit 1");
        assert_eq!(evs.last(), Some(&DownloadEvent::Failed("[x] boom".into())));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn exit_without_error_line() {
        let (evs, _b, _r) = run_fake("noerr", "exit 3");
        match evs.last() {
            Some(DownloadEvent::Failed(m)) => assert!(m.contains("exit"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn missing_tool() {
        let base = tdir("missing");
        let mut job = start(
            req(&base.join("dest")),
            tools(PathBuf::from("/nonexistent/yt-dlp")),
            base.join("root"),
            egui::Context::default(),
        );
        let evs = drain(&mut job);
        match evs.last() {
            Some(DownloadEvent::Failed(m)) => assert!(m.contains("not found"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    fn cancel_run(name: &str, prefix: &str) -> (Vec<DownloadEvent>, Duration, PathBuf, PathBuf) {
        let base = tdir(name);
        let body = format!(
            "{prefix}\nsleep 60 &\necho $! > {b}/pid\necho 'KHZ|T|x'\nwait",
            b = base.display()
        );
        let script = fake(&base, "yt-dlp", &body);
        let root = base.join("root");
        let mut job = start(
            req(&base.join("dest")),
            tools(script),
            root.clone(),
            egui::Context::default(),
        );
        let mut evs = Vec::new();
        let t0 = Instant::now();
        while evs.is_empty() {
            assert!(t0.elapsed() < Duration::from_secs(10));
            for p in job.poll() {
                if let Poll::Event(e) = p {
                    evs.push(e);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(evs[0], DownloadEvent::Title("x".into()));
        let t = Instant::now();
        job.cancel();
        evs.extend(drain(&mut job));
        (evs, t.elapsed(), base, root)
    }

    #[test]
    fn cancel_kills_group() {
        let (evs, took, base, root) = cancel_run("cancel", "");
        assert_eq!(evs.last(), Some(&DownloadEvent::Cancelled));
        assert!(took < Duration::from_secs(2), "{took:?}");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        let pid: i32 = std::fs::read_to_string(base.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(gone(pid), "sleep {pid} still alive");
    }

    #[test]
    fn cancel_escalates_to_kill() {
        let (evs, took, base, root) = cancel_run("escalate", "trap '' TERM");
        assert_eq!(evs.last(), Some(&DownloadEvent::Cancelled));
        assert!(took >= KILL_AFTER, "{took:?}");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        let pid: i32 = std::fs::read_to_string(base.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(gone(pid), "sleep {pid} still alive");
    }

    #[test]
    #[ignore = "network: needs KHZ_DL_URL and a real yt-dlp"]
    fn real_ytdlp_audio() {
        let Ok(url) = std::env::var("KHZ_DL_URL") else {
            return;
        };
        let base = tdir("real");
        let mut r = req(&base.join("dest"));
        r.url = url;
        let mut job = start(
            r,
            Tools::default(),
            base.join("root"),
            egui::Context::default(),
        );
        let evs = drain(&mut job);
        match evs.last() {
            Some(DownloadEvent::Done { files }) => {
                assert_eq!(files.len(), 1, "{evs:?}");
                assert!(files[0].exists());
            }
            other => panic!("{other:?}"),
        }
    }
}
