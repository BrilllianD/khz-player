# A4: endless 1 s saves when the DB is missing or a save fails

If save_playlist fails, dirty never clears, so logic re-arms mark_cfg and writes config.toml every second forever, plus a 250 ms repaint loop (src/app.rs:883-891, 945-960).

Fix: clear dirty on failure and toast once (save_failed flag, reset on success); skip the dirty check entirely when there is no DB.
