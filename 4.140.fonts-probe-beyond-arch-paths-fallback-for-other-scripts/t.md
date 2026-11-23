# Fonts: probe beyond Arch paths, fallback for other scripts

The font probe looks only at `/usr/share/fonts/TTF/...`, so on Ubuntu, Debian and Fedora, or
with a Nerd Font under `~/.local/share/fonts`, the promised default font and its icon glyphs are
lost (abs Y18). The chain has no fallback for CJK, Thai or Arabic, so such titles render as
boxes (real Y36).

- Where: `src/fonts.rs:8-22,34,46-51`, `README.md:149`
- Done when: with the font only in `~/.local/share/fonts` the startup log names that file; a
  fallback font is appended to both families and a Japanese title has no replacement glyphs.
