# A15: no recovery when the output device disappears or fails to open

Nothing reopens output after a device error (src/audio/output.rs:134, src/audio/mod.rs:70-77); each flush then stalls 100 ms (engine.rs:130).

Fix: Shared.device_lost atomic set in the error callback; app reopens output and sends Command::SetOutput { ring, rate }.
