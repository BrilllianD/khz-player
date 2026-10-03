use std::path::PathBuf;

use rmp::{app, config, ui};
use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("rmp=info")),
        )
        .init();

    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let cfg = config::Config::load();
    let size = app::window_size(&cfg);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rmp")
            .with_app_id("rmp")
            .with_inner_size(size)
            .with_min_inner_size([ui::MAIN_W, ui::MAIN_H])
            .with_decorations(false)
            .with_drag_and_drop(true),
        persist_window: false,
        // With vsync, eglSwapBuffers waits for a Wayland frame callback, which
        // Hyprland never sends to windows on hidden workspaces: the UI thread
        // stalls, misses pings and gets an "Application Not Responding" dialog.
        // Frame rate is already capped by our own repaint requests.
        glow_options: eframe::egui_glow::GlowConfiguration {
            vsync: false,
            ..Default::default()
        },
        ..Default::default()
    };
    eframe::run_native(
        "rmp",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, cfg, args)))),
    )
}
