# A17: scanner lists formats symphonia cannot decode; engine thread blocks on prefetch

Scanner accepts opus/wv/ape/mpc (src/library/tags.rs:11-13) but symphonia features cannot decode them; alac, aiff, caf, mkv, opt-simd not enabled. Also PrefetchNext opens the decoder synchronously on the engine thread (engine.rs:285), EQ denormals in silence (dsp.rs:83-89), the error callback logs on every xrun (output.rs:134).

Fix: align the extension list with enabled features; add the missing features; open prefetch off-thread; flush denormals; rate-limit xrun logs.
