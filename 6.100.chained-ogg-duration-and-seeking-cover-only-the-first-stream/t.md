# Chained Ogg: duration and seeking cover only the first stream

Found while fixing A14 (622b742). In a chained Ogg file, info.duration covers only the first logical stream, so the time display runs past the shown duration in later streams and SeekRel clamps to the first stream's length. symphonia's Ogg seek only works inside the current physical stream (phys_byte_range), so after a boundary you cannot seek back into an earlier stream. Rare in local files (mostly radio rips). Possible fix: sum durations by scanning the chain at open, or treat each link as its own playlist entry.
