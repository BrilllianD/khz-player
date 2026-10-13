# A25: second instance runs without MPRIS and shares config/DB

If the bus name is taken, a second khz-player starts anyway with no MPRIS and writes the same config and DB (src/mpris.rs:80-84).

Fix together with B1 (single instance + enqueue).
