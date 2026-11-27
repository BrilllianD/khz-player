# Download flush: keep the pending batch when upsert_one fails

Slice 6 review minor (src/app/downloads.rs:362): if db::upsert_one fails partway through a pending flush, the rest of the taken batch is dropped and load_tracks is skipped, so rows already upserted stay invisible in App.library until the next reload. Files are still in the playlist and on disk; a later scan picks them up. Fix: continue past the failing file (toast it) and always reload after a flush that upserted anything.
