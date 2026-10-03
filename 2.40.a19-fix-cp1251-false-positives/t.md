# A19: fix_cp1251 false positives

fix_cp1251 (src/library/tags.rs:51-73) rewrites short Latin-1 strings and tags from formats that are always UTF-8.

Fix: only apply to ID3v1/ID3v2 Latin-1 frames, require a minimum length / Cyrillic ratio. Unit tests.
