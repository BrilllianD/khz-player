## 2026-10-05T13:53:51Z — Bronnikov Aleksandr

on it

## 2026-10-05T13:57:59Z — Bronnikov Aleksandr

8047ad1: output config scored by channels then format with default config last; build arms for all cpal PCM formats (I8/I24/I64/U8/U24/U32/U64/F64); fresh ring/tap/CallbackState per attempt, no-op probe removed; per-frame gain ramp (10 ms full range) via dsp::approach; dsp::soft_clip replaces hard clamp. tests 73 -> 77 (75 passed + 2 ignored)

