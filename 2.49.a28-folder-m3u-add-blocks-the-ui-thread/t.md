# A28: folder / M3U add blocks the UI thread

Adding a folder or M3U runs walkdir and tag reads on the UI thread (src/app.rs:445-479, startup :140-144).

Fix: reuse the scanner-thread pattern and stream results back.
