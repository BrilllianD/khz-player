# A23: library grouping case mismatch and egui ID clash

Library sorts by Unicode-lowercase key (src/ui/library_panel.rs:71) but groups adjacent rows with eq_ignore_ascii_case (:101, :114), so Cyrillic case variants ("Кино" / "КИНО") sort together yet split into two groups.

Fix: group on the same Unicode-lowercase key used for sorting.

Done in 0.1.2 tree rewrite, no longer applies: CollapsingHeader id clash (tree is custom-painted), search-forced open state sticking (now derived from the searching flag each frame).
