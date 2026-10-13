# B1: single instance + enqueue

main.rs (fn main, before eframe::run_native) probes org.mpris.MediaPlayer2.khz-player on the session bus; if present, forward args via OpenUri and exit. Register an OpenUri handler in mpris.rs (Player::builder, mpris.rs:99) -> MprisAction::OpenUri -> add_path (app.rs:475). Enables Exec=khz-player %F. Fixes A25.
