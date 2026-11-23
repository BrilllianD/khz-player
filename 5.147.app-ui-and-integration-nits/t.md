# App, UI and integration nits

The blue app, UI and integration items of the realization note and the blue list of the
abstraction note (naming, small seams, MIME lists kept twice, `Theme::name`, `publish = false`,
tarball reproducibility, `install.sh` arguments). `Theme::name` and other dead items go to task
70 when that task is taken.

- Where: `src/app/playlists.rs:140,295`, `src/app/eq.rs:42,105`, `src/app/mod.rs:490`,
  `src/shortcuts.rs:112`, `src/playlist.rs:336-341,476-497`, `src/ui/playlist_panel.rs:110-150`,
  `src/ui/library_panel.rs:250-294`, `src/ui/cover.rs:18-31,75`, `src/mpris.rs:88,221,394,435`,
  `scripts/package.sh:48`, `scripts/install.sh:14`, `Cargo.toml:1`
- Done when: each item in those blue lists
  (`~/mem/projects/khz-player/notes/khz-player-realization-review.md`,
  `~/mem/projects/khz-player/notes/khz-player-abstraction-review.md`) is fixed or struck with a
  reason.
