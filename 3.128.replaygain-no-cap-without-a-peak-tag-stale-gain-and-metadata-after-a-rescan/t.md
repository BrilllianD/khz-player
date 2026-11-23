# ReplayGain: no cap without a peak tag, stale gain and metadata after a rescan

README says "peaks are never clipped", but the cap applies only when a peak tag exists;
untagged tracks get up to +30 dB by config (real Y5). After a scan or tag fill the playlist
entries get new `replay_gain`, but the prefetched next track keeps the old gain and MPRIS keeps
the old metadata; the v6 upgrade rescan exists to load these tags (real Y24). The refresh loop
is written twice (real Y30).

- Where: `src/audio/shared.rs:138-152`, `README.md:14`, `src/app/events.rs:234-241,253-271`,
  `src/app/playlists.rs:95,113-132`
- Done when: `replay_gain_factor(&ReplayGain::default(), RG_TRACK, 6.0, 6.0) <= 1.0` or the
  README is reworded and a test pins the rule; a scan `Done` that changes the next row's gain
  sends `PrefetchNext` with it and `MprisUpdate::Track` for the playing one; one refresh helper
  takes the filter.
