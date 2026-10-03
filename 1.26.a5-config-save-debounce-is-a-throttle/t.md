# A5: config save debounce is a throttle

mark_cfg uses get_or_insert_with (src/app.rs:238), so the timer starts at the first change and a long slider drag saves every second.

Fix: true debounce (reset on each change) with a 10 s cap from the first change. Pure should_save(first, last, now) with a unit test.
