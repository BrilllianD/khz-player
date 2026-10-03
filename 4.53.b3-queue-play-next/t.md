# B3: queue / play next

VecDeque<usize> on Playlist consulted first in peek_next/next (playlist.rs:294,324), remapped on remove/move (new_pos logic :208-211). Queue number shown in the row (playlist_panel.rs:152-158); Enqueue in the jump dialog (jump_dialog.rs:85).
