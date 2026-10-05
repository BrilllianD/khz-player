# Check: rmp starts with PipeWire stopped and recovers when it starts

A15 (437e92b). Stop PipeWire (systemctl --user stop pipewire pipewire-pulse wireplumber pipewire.socket pipewire-pulse.socket), start rmp: expect 'Audio output unavailable'. Start PipeWire again: within about 2 s rmp shows 'Audio output restored' and play works.
