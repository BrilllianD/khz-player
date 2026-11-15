## 2026-10-21T17:29:27Z — Bronnikov Aleksandr

Failed on 2026-10-21 (release build of bcf4d22): unplugging the USB headset (Logitech G PRO X) while playing, PipeWire moved the stream to the laptop speakers, sound came out of them briefly and then stopped. No toast appeared and the debug log has nothing after startup. Pressing Pause and then Play brought the sound back on the speakers. Plugging the headset back in while paused and pressing Play played through the headset. Same silent-stall mechanism as task 113; re-run after it is fixed.

## 2026-11-15T05:47:53Z — Bronnikov Aleksandr

Re-checked 2026-11-15 on the release build of 7cd2827 with dbus-monitor: on unplug, WirePlumber itself (pid of /usr/bin/wireplumber, 0.5.18) calls MPRIS Pause on khz-player. That is its stock linking.pause-playback hook ('Pause media players if their target sink is removed', scripts/linking/mpris-pause.lua). The stream moves to the laptop speakers, the player stays paused, Play resumes on the speakers; replugging does not auto-resume (WirePlumber never sends Play). No stall, no error, no reopen needed: this is the expected behaviour, and the 2026-10-21 'sound then silence, Pause/Play recovers' was this pause, not a stall.

