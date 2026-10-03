# A12: volume wheel moves several percent per notch

The VOL slider wheel handler uses smooth_scroll_delta (src/ui/main_panel.rs:156-159), which spreads one notch over several frames, each stepping the volume.

Fix: use raw_scroll_delta and step 2 % per notch.
