# A11: stuck drag flag and uncapped request_repaint with vsync off

The playlist drag flag only clears on any_released (src/ui/playlist_panel.rs:231-240); a release outside the window leaves it set, and it calls request_repaint() every frame, which spins at full speed now that vsync is off (src/main.rs:30).

Fix: clear the flag when no pointer button is down and on tab switch / panel hide; use request_repaint_after(16 ms); skip repaint scheduling while minimized.
