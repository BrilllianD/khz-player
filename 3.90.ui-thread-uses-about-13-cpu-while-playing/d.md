## 2026-10-04T05:00:24Z — Bronnikov Aleksandr

Profiling with perf on a symbol build before any fix.

## 2026-10-04T05:32:53Z — Bronnikov Aleksandr

Profile (perf, frame-pointer build, 15 s windows, library panel closed).
Playing: top 10.5-11.5 % CPU. Stopped: 0.2 %, UI thread idle. So the whole cost is the 30 fps repaint while playing.
Threads while playing: main 74 %, audio-engine 12 %, rmp:gl0 (Mesa) 11 %, rest 3 %.
Main thread, inclusive: ui::show 30 % (playlist_panel 15.6: rows 7.8, footer 4.3, tabs 2.4; main_panel 11.1: open-path TextEdit 2.4, spectrum 1.1), Context::tessellate 16.4 %, egui_glow paint 12.2 %, swap_buffers/dri_flush 18.3 %, App::logic 2.6 % (Spectrum::update 1.4), event loop and pass bookkeeping the rest.
Our own code is small: per-row display_name/format! about 2 %, nothing hot. The cost is egui rebuilding and tessellating the whole window each frame plus the driver. The lever is frame count, not a hot spot.

## 2026-10-04T05:53:33Z — Bronnikov Aleksandr

Fixed in ba56b71: new config value animation_fps (10-60, default 30) sets the repaint rate while playing. The profile showed no hot spot of ours, so frame count is the lever. top while playing: 10.5-11.5 % at 30 fps, 6.5-8.0 % at 20 fps.

