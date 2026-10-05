# A15: no recovery when the output device disappears or fails to open

Nothing reopens output after a device error (src/audio/output.rs:134, src/audio/mod.rs:70-77); each flush then stalls 100 ms (engine.rs:130).

Fix: Shared.device_lost atomic set in the error callback; app reopens output and sends Command::SetOutput { ring, rate }.

## Plan (from PLAN_TODO.md)

Files: `src/audio/output.rs`, `src/audio/shared.rs`, `src/audio/mod.rs`, `src/audio/engine.rs`, `src/app.rs`.

Problem:
- When the device disappears (USB DAC or Bluetooth unplugged, PipeWire restarted), the stream errors once and stops pulling. Nothing reopens it, the engine waits 100 ms in every `flush` (`engine.rs:130`), and position freezes.
- If output fails at startup, rmp stays silent until restarted (`mod.rs:73-81`, toast in `app.rs:228`).

Change:
- `Shared.device_lost: AtomicBool`. The error callback sets it for every kind except `Xrun` and `RealtimeDenied`. It also needs the egui `Context` to request a repaint: pass a clone into `output::open`.
- `AudioHandle::reopen(&mut self) -> anyhow::Result<()>`: calls `output::open(shared.clone())` again, swaps `_output` and `tap`, and sends the new ring producer and rate to the engine via `Command::SetOutput { ring: rtrb::Producer<f32>, rate: u32 }`.
  - `rtrb::Producer` is not `Clone`, but `Command` derives `Clone`. Grep for clones of `Command`; if none, drop the derive, otherwise wrap the producer in `Arc<Mutex<Option<_>>>`.
  - Also clear `device_lost` and `init_error`.
- Engine `SetOutput`: remember `pos = self.position()`, swap `ring`, set `out_rate` and `shared.out_rate`. Rebuild `cur.resampler` and drop `next` (the app re-prefetches after the `Seeked` event; check, or call `refresh_prefetch` from the app after reopen). Reset `pushed_total = consumed()`, clear `transitions`, then `seek(pos)` if a track is loaded, so position and `track_start` are consistent in the new rate.
- Flush must not stall on a dead device: in `flush`, skip the wait loop when `device_lost` is set.
- App `logic`: when `shared.device_lost` is set, or `_output` is `None` (startup failure), try `reopen` at most every 2 s (`last_reopen: Instant`). First failure toasts "Audio device lost, retrying". Success toasts "Audio output restored" and keeps the state: paused stays paused, playing continues.
- Spectrum: `Spectrum::new(rate)` must follow a rate change. Add `Spectrum::set_rate` or rebuild it.

Tests:
- Engine `set_output_switches_ring_and_rate`: in `test_engine()`, load a WAV and fill, then send a new ring at 44100. `out_rate` changed, position stayed within 10 ms, new frames go to the new ring.
- `flush` no-wait path: with `device_lost` set, `flush` returns in under 5 ms.

Manual (file as `user-manual` tasks), rmp must recover within about 2 s without a restart:
1. Play through a USB or Bluetooth device and unplug it.
2. `systemctl --user restart pipewire pipewire-pulse wireplumber`.
3. Start rmp with PipeWire stopped, then start PipeWire.

Risk: under PipeWire, cpal 0.18 may report device loss as `StreamInvalidated` or `BackendError` rather than `DeviceNotAvailable`. Treating every non-xrun error as lost is deliberately broad.

Depends on the xrun counter from A17 (task 38, part 2c). Release: 0.1.3.
