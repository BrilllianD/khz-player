# A6: load/stop during a pending gapless transition mislabels the current track

Engine::flush emits Advanced for pending transitions (src/audio/engine.rs:138-140). load(C) and stop() call flush after the app already set current = C, so on_track_started(gapless) moves the pointer to B or past C (src/app.rs:674-686).

Fix: only seek drains transitions and emits Advanced; load and stop clear them silently. Engine test with synthetic WAVs.
