# Check: playback recovers after unplugging the audio device

A15 (437e92b). Play through a USB or Bluetooth device and unplug it. Expect the toast 'Audio device lost, retrying', then within about 2 s 'Audio output restored', and playback continues on the new default device at the same position. Paused must stay paused. The spectrum must look right afterwards, and gapless into the next track must still work. No repeated toasts while the device stays gone.
