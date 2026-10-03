# rmp

Winamp-classic style music player in Rust. Design: `PLAN.md`.

Before committing: `cargo test && cargo clippy --all-targets -- -D warnings`.

## Tasks (yman)

Tasks live in `.yman/` (local only, no remote yet).

    yman ls -n 10 --assignee -                        # pick: unassigned, best priority first
    yman show <id> -n 3                               # read: header, body, last 3 comments
    yman set <id> --status doing -a <me> -m "on it"   # claim: status, assignee, note, one commit
    yman done <id> -m "what changed"                  # close, explain
    yman add "Title" -m "body" -a <me> --relate <id>  # follow-up, linked to its parent

Never `edit` or `-e` (opens an editor); change a body with `yman set <id> --body "..."`.
`rm` and `tags rm` need `-f` off a terminal.
