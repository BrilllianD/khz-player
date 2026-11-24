## 2026-11-24T00:56:26Z — Bronnikov Aleksandr

Decided by the user 2026-11-24: the release tarball stays on the Ubuntu 22.04 image and is built without the video feature (--no-default-features plus the other default features); source and distro builds keep video. CI test job moves to ubuntu-24.04 in this slice.

## 2026-11-24T01:17:46Z — Bronnikov Aleksandr

on it

## 2026-11-24T01:21:31Z — Bronnikov Aleksandr

dc851b2: VideoPlayer over libmpv2, feature video, CI 24.04, tarball without video

## 2026-11-24T01:25:07Z — Bronnikov Aleksandr

841a207: fix wave, load start=none for zero start, mapper restarted gate removed, 20/20 video:: runs

