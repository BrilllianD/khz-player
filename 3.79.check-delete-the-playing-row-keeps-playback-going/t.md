# Check: delete the playing row keeps playback going

Fix A1 (v0.1.1). Play row 5 of a list, select it, press Del. Expected: the track keeps playing; when it ends, the old row 6 (now row 5) plays and is highlighted. Repeat with the last row: Repeat off stops at the end, Repeat all wraps to row 1. Not run by Claude: needs the GUI and audio.
