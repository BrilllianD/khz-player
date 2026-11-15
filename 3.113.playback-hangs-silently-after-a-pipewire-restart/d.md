## 2026-10-21T17:27:05Z — Bronnikov Aleksandr

No workaround inside the app: Pause/Play and Stop/Play (or double-clicking a track) after the restart all stay silent. Only restarting khz-player recovers.

## 2026-10-21T17:29:27Z — Bronnikov Aleksandr

Also triggered by unplugging the default USB sink while playing (manual check 97): PipeWire moves the stream to the next sink, a moment of sound, then silence with no error and no log line. Unlike the server restart, Pause then Play recovers here. The stall watchdog should cover both cases.

## 2026-11-15T05:12:35Z — Bronnikov Aleksandr

on it

## 2026-11-15T05:19:01Z — Bronnikov Aleksandr

b15ce16: StallWatch in engine sets device_lost; output::retire drops old stream detached

## 2026-11-15T05:23:11Z — Bronnikov Aleksandr

7cd2827: retired stream error callback no longer sets device_lost; reopen keeps device_lost set until open returns so the stall watch cannot trip on the old ring

