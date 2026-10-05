## 2026-10-05T13:49:32Z — Bronnikov Aleksandr

on it

## 2026-10-05T13:53:00Z — Bronnikov Aleksandr

622b742: decoder handles ResetRequired via reset_track (re-select audio track, rebuild codec); engine fill rebuilds the resampler on rate change (rebuild_resampler, reusable by A15); Resampler::in_rate. Tests 71 -> 73 (72 run + 1 new ignored chained_ogg_real_file, checked locally on ffmpeg-built chains 44.1k stereo + 32k mono and 48k + 22.05k + 44.1k).

