# Slice 4: Download module (yt-dlp subprocess)

Part C slice 4 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 4'); constraints: outline-constraints.md next to it.

Boundary: `src/download.rs`
Depends on: Slice 2
1. `download::start` with parse, cancel and fake-tool tests, W = 4 (files 3: `src/download.rs`
   new, `src/lib.rs`, `Cargo.toml` (`libc`) → 1; +2 new interface; +1 new test infrastructure,
   the fake yt-dlp script through `Tools`). Scope: argv builder, `validate_url`, `parse_line`,
   process-group cancel, per-job temp dir, `probe_tools`. Consumes `job::spawn`. Produces the
   download API in the constraints file.
ΣW = 4. Produces: `download::{start, Request, Tools, ToolStatus, probe_tools, validate_url,
temp_root, DownloadEvent}`.
Must verify with one network run by hand, recorded in the plan's report: which stream the
progress lines go to under `--print` (needs `--no-simulate --progress`, because `--print`
implies quiet and simulate); how missing numeric fields render in `--progress-template` (`NA`);
the exact postprocessor names for `Stage`; whether `after_move` fires for a file that already
exists; whether thumbnail embedding into Ogg Opus needs mutagen on this machine and what yt-dlp
does without it (if the job fails, drop `--embed-thumbnail` for Opus and note it); that a site
offering only AAC yields `.m4a` with `-x` and no `--audio-format`.
