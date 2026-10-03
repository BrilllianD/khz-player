# A13: accurate-seek trim mixes time-base ticks with frames

src/audio/decoder.rs:146-150 subtracts pts in time-base ticks from a frame count. Wrong trim whenever time base != sample rate.

Fix: convert until - pts through time_base to frames. Test with a time base != rate.
