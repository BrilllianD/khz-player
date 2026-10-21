## 2026-10-21T16:54:16Z — Bronnikov Aleksandr

Seen during manual check 95 on 2026-10-21: a chain of a 20 s 44100 Hz Vorbis stream and a 20 s 32000 Hz stream shows 0:14 both in the library (duration_ms 14512) and in the player. 14.512 s = 640000 / 44100: the last granule position comes from the second stream (20 s x 32000) but is divided by the first stream's rate. So the duration is not just first-stream-only; it mixes streams. Test file recipe: ffmpeg -ar 44100 a.ogg, ffmpeg -ar 32000 b.ogg, cat a.ogg b.ogg > chained.ogg.

