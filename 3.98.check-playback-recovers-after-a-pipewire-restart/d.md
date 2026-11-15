## 2026-10-21T17:27:05Z — Bronnikov Aleksandr

Failed on 2026-10-21 (release build of bcf4d22): after the restart while playing, khz-player stays silent with the time frozen and shows no toast; Pause/Play and Stop/Play do not recover it. Cause and fix idea in task 113. The paused variant was not run. Re-run after 113 is fixed.

## 2026-11-15T05:47:53Z — Bronnikov Aleksandr

Passed 2026-11-15 on the release build of 7cd2827. Playing: position froze ~1 s, log 'audio output stalled: no callback for 1s, reopening', new stream on PRO X 25 ms later, 'Audio output restored', playback continued from the same position. Paused: stayed Paused 3 s after the restart; on Play ~1 s of silence, then the watchdog reopened and playback continued (a dead stream is only noticed once playing; see 115). The user heard a click at the moment the server went away; that cut is outside the app's control.

