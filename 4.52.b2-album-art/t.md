# B2: album art

tags.rs:88 read_cover_art(false) -> on-demand tags::cover(path) with cover.jpg/folder.jpg fallback, cache ~/.cache/rmp/art/<hash>.jpg. MPRIS artUrl first (mpris.rs:171-183, MprisUpdate::Track field); in-app display later via the image crate.
