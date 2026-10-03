# A27: dropping below the last row cannot place at the end

The drop target past the last playlist row is not handled (src/ui/playlist_panel.rs:181-189), so you cannot drag a block to the end.

Fix: treat a drop below the last row as gap len.
