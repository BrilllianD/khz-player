# Check: playback recovers after a PipeWire restart

A15 (437e92b). While playing, run systemctl --user restart pipewire pipewire-pulse wireplumber. Playback resumes within about 2 s at the same position. Repeat while paused: it stays paused and resumes normally on play.
