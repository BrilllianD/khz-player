# A16: output format fallback gaps, stepped gain, hard clip

Format fallback misses I24/U24/I8/U8/F64 (output.rs:78-83), picks a rate without checking channel count (:45-56), probes each config twice (:115-137); volume/balance step per block (:221); hard clamp after EQ (:231).

Fix: add the format arms, per-sample gain ramp, soft clip.
