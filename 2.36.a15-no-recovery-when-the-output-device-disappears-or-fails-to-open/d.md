## 2026-10-05T14:01:21Z — Bronnikov Aleksandr

on it

## 2026-10-05T14:07:24Z — Bronnikov Aleksandr

437e92b: Shared.device_lost set by the stream error callback (all kinds except Xrun/RealtimeDenied/DeviceChanged) and on failed start; flush skips the ack wait while lost; AudioHandle::reopen + Command::SetOutput (engine swaps ring, rebuilds resampler, cancels prefetch, seeks back; pending gapless successor restarts at 0); app retries every 2 s with toasts, rebuilds spectrum, re-prefetches. Tests 83 -> 86 (84 passed + 2 ignored). Manual checks still open: unplug USB/BT, restart pipewire, start with pipewire stopped.

