## 2026-10-21T17:29:27Z — Bronnikov Aleksandr

Failed on 2026-10-21 (release build of bcf4d22): unplugging the USB headset (Logitech G PRO X) while playing, PipeWire moved the stream to the laptop speakers, sound came out of them briefly and then stopped. No toast appeared and the debug log has nothing after startup. Pressing Pause and then Play brought the sound back on the speakers. Plugging the headset back in while paused and pressing Play played through the headset. Same silent-stall mechanism as task 113; re-run after it is fixed.

