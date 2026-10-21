## 2026-10-21T17:13:00Z — Bronnikov Aleksandr

Partial check by hand on 2026-10-21 (release build of bcf4d22): gapless by ear passes (see task 15, sample-exact pairs incl. 44100 Hz through the 48000 Hz resampler). With RUST_LOG=khz_player=debug the xrun line appears at most every 10 s (17:02:02, :12, :22, :42). Paused with EQ on (nine bands +12 dB): top -H shows 0.0% for every khz-player thread (audio-engine, cpal_alsa, data-loop) over 4 s. Not yet checked: playback from a slow or network disk.

