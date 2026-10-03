# B1: single instance + enqueue

main.rs:13 probes org.mpris.MediaPlayer2.rmp; if present, forward args via OpenUri and exit. connect_open_uri (mpris.rs:149) -> MprisAction::OpenUri -> add_path (app.rs:442). Enables Exec=rmp %F. Fixes A25.
