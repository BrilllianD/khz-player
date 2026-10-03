# A8: next/prev do nothing after deleting the playing playlist

delete_playlist sets playing_list = usize::MAX (src/app.rs:556-557); next/prev read playing_list before playing() repairs it (src/app.rs:263-284), and the TrackEnded handler has the same guard (src/app.rs:649-656).

Fix: call playing() first, then read playing_list; delete_playlist points playing_list at the new active list and sends PrefetchNext(None).
