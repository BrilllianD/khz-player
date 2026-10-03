# Release profile: strip and thin LTO

target/release/rmp is 27.7 MB unstripped; add [profile.release] strip = true, lto = "thin" and compare size/build time.
