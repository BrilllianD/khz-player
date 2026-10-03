# Check: settings save debounce during a volume drag

Fix A4/A5 (v0.1.1). Run `inotifywait -m ~/.config/rmp` and drag the VOL slider for 5 s, then for 15 s. Expected: one config.toml write about 1 s after letting go; during the long drag one extra write at about 10 s. No writes every second while idle. Not run by Claude: needs the GUI.
