# A3: broken config.toml / eq_presets.toml overwritten by defaults

A parse error falls back to defaults (src/config.rs:113-117, src/eq_presets.rs:81-84) and the next save overwrites the user's file. eq_presets::save_user is not atomic (src/eq_presets.rs:89-95). Nothing is clamped on load.

Fix: rename an unparsable file to *.broken and toast the error; add Config::clamp (volume, balance, EQ range, window height); make save_user write tmp + rename. Split load_from/save_to so tests use a temp dir.
