# Output: System default follows a default-sink change made in the mixer

Follow-up from the 0.4.0 outline (Q1, 2026-11-15). With the cpal pulseaudio backend (0.4.0 slice 6), the stream is bound to the sink it opened on, so with 'System default' selected the player does not follow a default-sink change made in a mixer (wpctl, pavucontrol) until the next reopen. Out of scope for 0.4.0. Possible fix: watch the default sink (pulse server-info subscription or wpctl/PipeWire metadata) and reopen when it changes while 'System default' is selected.
