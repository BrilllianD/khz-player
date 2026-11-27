# Jobs modal: long download title pushes Cancel out of the modal

Slice 7 review minor (src/ui/jobs_dialog.rs:33): Label::new(row_label(r)).truncate() in the first Grid column takes all offered width, so a long yt-dlp title can push the state column and the Cancel button past the 520 px modal. Cap the first column width (set_max_width on the cell or a grid column width) and check by hand with a long title.
