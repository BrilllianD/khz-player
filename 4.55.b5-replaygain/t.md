# B5: ReplayGain

Read lofty ItemKey::ReplayGain* in tags::read (tags.rs:105-117); v5 migration with columns; apply per-track gain in engine fill after resampling (engine.rs:336) so it switches sample-exact. Config: off/track/album, preamp, peak guard.
