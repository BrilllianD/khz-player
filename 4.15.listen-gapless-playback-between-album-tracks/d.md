## 2026-10-21T17:08:30Z — Bronnikov Aleksandr

Checked by hand on 2026-10-21 (release build of bcf4d22): sample-exact splits (music 44100 Hz, music 2 44100 Hz, 440 Hz tone at 44100 and 48000 Hz) play across the track boundary with no gap or click; the 44100 Hz pairs go through the 48000 Hz resampler. Pitch matches mpv.

