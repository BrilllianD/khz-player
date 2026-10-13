# A29: EQ AUTO bookkeeping and small UI leftovers

EQ AUTO: a slider tweak is not stored, save_user_preset does not record the mapping, a user preset shadows a built-in with both shown selected, orphan eq_auto rows (src/app.rs:371-427, src/ui/eq_panel.rs:47,72,79,162). Also: Stop leaves the seek bar active (main_panel.rs:204, engine.rs:252); window and library width never persisted (app.rs:100, ui/mod.rs:73); odd startup tag-fill condition !library.is_empty() (app.rs:141).

(scroll_to yanking the view was fixed in 0.1.2: it scrolls only when the row is out of view.)
