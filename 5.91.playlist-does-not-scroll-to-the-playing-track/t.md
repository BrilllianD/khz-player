# Playlist does not scroll to the playing track

On track change (auto-advance, Next/Prev, double-click) the track plays but the playlist view is not on it. Cause: show_rows takes its row stride from the caller's item_spacing.y (theme: 3.0), but the closure zeroes it and draws 17 px rows, so egui's stride is 20 and row*ROW_H is not the row's offset. Same stride mismatch in the library tree (blank tail, 3 px hops). Fix: zero the spacing before show_rows; scroll only when the playing row is out of view.
