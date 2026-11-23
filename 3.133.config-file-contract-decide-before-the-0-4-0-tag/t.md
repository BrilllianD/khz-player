# Config file contract: decide before the 0.4.0 tag

README invites editing `config.toml`, but every save re-serializes the struct: unknown keys,
comments and edits made while the player runs disappear silently (abs Y6). Key names mix unit
conventions (`[eq] preamp` in dB without a suffix against `[replaygain] preamp_db`; `w` against
`library_w`); the keys added in 0.4.0 can be renamed for free only until the tag (abs Y7).
Needs a decision by the owner before release task 119.

- Where: `src/config.rs:14-16,66,68,80,83,89,231`, `README.md:146`
- Done when: before the 0.4.0 tag one naming convention is chosen and a config test pins the
  key names; an unknown key produces a toast naming it, or README states the file is rewritten
  and must be edited only while the player is closed.
