## 2026-11-27T21:15:40Z — Bronnikov Aleksandr

Partly done by epic 148 (Part C): scanner and adder run on job::Job with progress, terminal event, cancel and end-on-disconnect (slice 2); src/ui has no audio.send/audio.shared, all engine calls go through src/app/transport.rs (slice 8); App::modal_open() feeds the shortcut gate (slice 6). Still open: Shared is pub outside audio (src/audio/mod.rs:6,70), settings write cfg directly (grep count 20 in src/ui), art::find has 2 call sites (two cover pipelines).

