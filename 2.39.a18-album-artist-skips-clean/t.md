# A18: album_artist skips clean()

src/library/tags.rs:113-116 stores album_artist without clean(), unlike the other tags.

Fix: pass it through clean().
