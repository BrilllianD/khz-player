# Slice 3: Download and video config

Part C slice 3 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 3'); constraints: outline-constraints.md next to it.

Boundary: `src/config.rs`
Depends on: none
1. `DownloadConfig`, `VideoConfig`, clamps, W = 3 (files 1 → 1; +2 new types). Scope: the two
   tables in the constraints file, defaults, `max_height` clamp, empty `folder` as `None`,
   round-trip and clamp tests. Produces `cfg.download`, `cfg.video`.
ΣW = 3. Produces: `Config::download`, `Config::video`.
