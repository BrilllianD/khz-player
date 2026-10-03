# Check: broken config.toml is kept as .broken

Fix A3 (v0.1.1). Back up ~/.config/rmp/config.toml, then `echo garbage >> ~/.config/rmp/config.toml` and start rmp. Expected: a toast about config.toml.broken; the .broken file holds the original text including library_roots; rmp starts with defaults. Restore the backup afterwards. Not run by Claude: touches the real config and needs the GUI.
