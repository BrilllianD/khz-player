# DB migrations v1/v4/v5 race on concurrent first start

src/library/db.rs migrate: the v1, v4 and v5 steps read user_version before a deferred BEGIN, so two instances starting at once on a fresh or old DB can both run them. v4/v5 look idempotent; v1 uses CREATE TABLE without IF NOT EXISTS, so the second instance fails with 'table already exists' (next start works). The v6 step already does BEGIN IMMEDIATE + re-read of user_version (commit 0ee1fa9); apply the same pattern to all steps. Found in the 0.4.0 blue-minors re-review.
