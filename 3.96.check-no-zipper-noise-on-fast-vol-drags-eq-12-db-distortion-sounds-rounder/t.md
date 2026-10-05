# Check: no zipper noise on fast VOL drags; EQ +12 dB distortion sounds rounder

A16 (8047ad1). While music plays, drag VOL and balance fast back and forth: no zipper noise or crackle. Set all EQ bands to +12 dB at preamp 0: the distortion should sound rounder (soft clip), with no harsh crackle. On other hardware (USB DAC, HDMI, Bluetooth) the startup log line 'audio output: ...' shows a sane format and channel count; rejected configs are logged at debug as 'stream config ... failed'.
