## 2026-10-21T16:40:14Z — Bronnikov Aleksandr

Checked by hand on 2026-10-21 (release build of bcf4d22): dropping files or a folder onto the window does nothing. No GUI file manager is installed, so the drag source was a browser file:// listing. The app uses eframe 0.36.2 on winit 0.30.13; winit does not deliver DroppedFile on Wayland, so handle_drops (src/app/events.rs:303) never sees a path. Follow-up filed.

