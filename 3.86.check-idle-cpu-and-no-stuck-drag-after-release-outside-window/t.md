# Check: idle CPU and no stuck drag after release outside window

Fix A10/A11 (v0.1.1). With the playlist open and playback stopped, `top -p $(pgrep rmp)` stays near 0 %. Start dragging a row, move the pointer outside the window, release, come back: the next click selects normally and does not move rows, CPU returns to idle. Minimize while playing: CPU drops. Not run by Claude: needs the GUI.
