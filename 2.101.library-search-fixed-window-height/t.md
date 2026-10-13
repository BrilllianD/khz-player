# Library search: fixed window height

Typing in the library search makes the window jump in height with the number of found items (src/ui/library_panel.rs:173, tree/visible_rows ~238-290). Anchor the window by its top border and keep its height independent of the match count (fixed or min height, results scroll inside).

Origin: requested by user; needs agent to claim.
