# Playback hangs silently after a PipeWire restart

Found during manual check 98 on 2026-10-21 (release build of bcf4d22). While a track plays, systemctl --user restart pipewire pipewire-pulse wireplumber leaves khz-player silent with the time display frozen. No toast appears and the debug log has no line at all after the restart: no device-lost, no reopen, no xrun. wpctl status lists no khz-player stream afterwards.

Cause: output goes through cpal's ALSA backend on top of the pipewire-alsa plugin (threads cpal_alsa_out and alsa-pipewire). After the server goes away the plugin never signals an error; cpal_alsa_out sits in poll (wchan poll_schedule_timeout) forever, so the stream error callback never fires, shared.device_lost (src/audio/mod.rs) is never set, and the reopen loop in src/app/events.rs never runs.

Fix idea: a stall watchdog that does not depend on backend errors. While not paused and the ring holds frames, if shared.consumed has not advanced for about 1 s, treat the device as lost (set device_lost) so the existing reopen path takes over. Take care that a legitimately idle stream (stopped, paused, empty ring at end of playlist) never trips it, and that the old cpal stream can be dropped even though its callback thread is blocked in poll. Alternative or addition: a native PipeWire backend. Afterwards re-run manual checks 97, 98 and 99.
