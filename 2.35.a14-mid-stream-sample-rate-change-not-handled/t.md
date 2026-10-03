# A14: mid-stream sample-rate change not handled

ResetRequired is treated as EOF and a rate change is ignored (src/audio/decoder.rs:114,140, src/audio/engine.rs:334-336).

Fix: compare the decoder rate to the resampler input rate in fill and rebuild the resampler on mismatch; handle ResetRequired by resetting the decoder.
