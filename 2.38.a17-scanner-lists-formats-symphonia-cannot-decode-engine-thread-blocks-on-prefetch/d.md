## 2026-10-05T13:41:36Z — Bronnikov Aleksandr

on it

## 2026-10-05T13:48:45Z — Bronnikov Aleksandr

1412454, b4bde83, 2309d3d: symphonia alac/aiff/adpcm/mp1/mp2/opt-simd (mkv+caf dropped: full set was +1.29 MB, without them +0.95 MB); extension list matches decodable set (opus/wv/ape/mpc out, aif/aiff/aifc/mp1/mp2 in); EQ biquad state flushed below 1e-30; xruns counted in Shared::xruns and logged by the engine at most every 10 s; prefetch opens on a background thread with generation check, engine selects over commands and prefetch results. Tests 67 -> 71 (70 + 1 ignored).

