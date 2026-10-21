## 2026-10-21T17:41:10Z — Bronnikov Aleksandr

Checked by hand on 2026-10-21 (release build of bcf4d22), twice: with PipeWire and its sockets stopped, khz-player starts and shows 'Audio output unavailable' (snd_pcm_open: Host is down (112)), retries about every 2 s, and within 0.8-1.4 s of PipeWire starting shows 'Audio output restored'; Play then works.

