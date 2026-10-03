## 2026-10-03T18:33:28Z — Bronnikov Aleksandr

on it

## 2026-10-03T18:34:32Z — Bronnikov Aleksandr

Config::load/load_from and eq_presets::load_user_from move a bad file to *.broken and return the message, toasted by App::new. Config::clamp on load. Shared write_atomic for config and presets. 3 tests.

