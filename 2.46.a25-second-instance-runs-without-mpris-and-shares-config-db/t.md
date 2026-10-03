# A25: second instance runs without MPRIS and shares config/DB

If the bus name is taken, a second rmp starts anyway with no MPRIS and writes the same config and DB (src/mpris.rs:81-82).

Fix together with B1 (single instance + enqueue).
