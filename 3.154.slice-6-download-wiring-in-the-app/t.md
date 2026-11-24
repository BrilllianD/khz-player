# Slice 6: Download wiring in the app

Part C slice 6 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 6'); constraints: outline-constraints.md next to it.

Boundary: `src/app` (plus `src/shortcuts.rs` binding)
Depends on: Slice 3, Slice 4, Slice 5
1. FIFO download queue, events, library entry, prompt kind, action, W = 4 (files 5:
   `src/app/downloads.rs` new, `src/app/mod.rs`, `src/app/events.rs`, `src/shortcuts.rs`,
   `src/ui/mod.rs` (prompt submit arm only) → 2; +2 new interface, the App download API).
   Scope: queue with one running job, drain in `logic`, pending-while-scanning flush on
   `ScanEvent::Done`, destination rule, append to active playlist, `PromptKind::Download`,
   `Action::DownloadUrl` on Ctrl+L, `App::modal_open()` replacing the gate at
   `src/app/mod.rs:494-497`, tool probe on a job. Produces the App download API.
ΣW = 4. Produces: `App::{start_download, cancel_download, downloads, download_label,
tool_status, modal_open}`, `prompt_keep_video`, `jobs_open`.
