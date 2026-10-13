# Native file/folder dialog for adding

Adding files and folders needs a typed path (src/ui/main_panel.rs:277 app.add_path), which is uncomfortable. Use a native OS dialog (e.g. rfd crate, works on Wayland via portal): multi-select files and folder pick. Keep drag-drop. Must not block the UI thread (see task 49).

Origin: requested by user; needs agent to claim.
