# Check: unplayable file in a playlist is skipped

Fix A6/A7 (v0.1.1). Make a playlist of 3 files, delete or rename the middle file on disk, play from the first. Expected: toast naming the file, playback continues with the third, the broken row shows muted. A playlist of only broken files stops after one pass without spinning. Also: stopping or double-clicking another track in the last second of a track must not move the highlight past the chosen track. Not run by Claude: needs the GUI and audio.
