# UI thread uses about 13 % CPU while playing

Measured by the user during check 86: 13 % CPU on the UI thread while playing, library panel closed, Ryzen 7 4700U, 30 fps repaint. That is about 4 ms per frame. Audio threads are about 3 % together. Cause unknown; needs a profile, not guesses: install perf, build release with debug=line-tables-only and strip=false, then perf record -g -p $(pgrep -x rmp) for 10 s while playing. Suspects: spectrum update and draw, title scroll, egui tessellation, Mesa driver overhead with glow.
