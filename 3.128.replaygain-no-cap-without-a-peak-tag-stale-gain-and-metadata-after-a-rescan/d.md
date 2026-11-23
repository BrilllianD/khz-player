## 2026-11-23T19:46:09Z — Bronnikov Aleksandr

Found by the slice 3 reviewer of the 121-125 fix run (2026-11-23, at d947781): a finite but huge tagged gain with no peak tag (for example REPLAYGAIN_TRACK_GAIN = 800 dB) passes the is_finite filters in src/library/tags.rs parse_rg and src/audio/shared.rs replay_gain_factor; 10f32.powf then overflows to Inf, and Inf times a sample is Inf or NaN at the device (soft_clip passes NaN). The cap this task adds for the no-peak case must also bound the dB value before powf (clamp the summed dB to a sane range, and make the returned factor finite).

