# Treat '?'-only tags as missing (fall back to filename)

Бумбокс.mp3 has artist '???????' title '??,???' written lossily; tags::read should drop strings made only of '?', punctuation and spaces so display falls back to the file stem. Bump DB user_version to rescan.
