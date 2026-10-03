# A7: a file that fails to load stops playback instead of skipping

Decoder::open failure sends only Error and goes Stopped (src/audio/engine.rs:229-236); the app toasts and halts (src/app.rs:659-668).

Fix: new Event::LoadFailed { path, msg }. The app toasts, remembers the path in a failed set (used for the muted row colour) and skips to the next track, giving up after one pass over the list. Reset the counter on a successful start.
