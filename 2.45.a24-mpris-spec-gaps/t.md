# A24: MPRIS spec gaps

xesam:url not percent-encoded (src/mpris.rs:182); OpenUri advertised with no handler (:108); Position pushed every 500 ms only while playing (src/app.rs:789-794), not on pause/stop; Can* flags static (mpris.rs:102-106); SetPosition ignores trackid and length (:134, app.rs:779); Seek past the end clamps instead of Next (engine.rs:181-183).

Fix each; unit-test URL encoding and loop_status mapping.
