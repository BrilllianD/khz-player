# Check: khz-player starts with PipeWire stopped and recovers when it starts

A15 (437e92b). Stop PipeWire (systemctl --user stop pipewire pipewire-pulse wireplumber pipewire.socket pipewire-pulse.socket), start khz-player: expect 'Audio output unavailable'. Start PipeWire again: within about 2 s khz-player shows 'Audio output restored' and play works.
