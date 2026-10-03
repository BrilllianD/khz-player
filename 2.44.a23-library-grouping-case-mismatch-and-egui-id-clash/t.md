# A23: library grouping case mismatch and egui ID clash

Library groups by ASCII case but sorts by Unicode case; collapsing headers clash on equal labels (src/ui/library_panel.rs:65,95,108,218); search forces open(Some(true)) which sticks after the search is cleared (:219,226).

Fix: one Unicode-lowercase key for both; ids from the group key + parent; only force open while a query is active.
