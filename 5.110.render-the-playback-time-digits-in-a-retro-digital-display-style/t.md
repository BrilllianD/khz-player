# Render the playback time digits in a retro digital display style

Playback time readout (widgets::digits) is painted as seven-segment LCD cells: lit segments in theme.digits, unlit in 12% dim. Supports 0-9, ':', '-', space; right-aligned 10 px from the edge, scales down to fit. Pure helpers segments() and lcd_cells() are unit-tested. No toggle.
