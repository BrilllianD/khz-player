# A20: scanner holds the SQLite write lock through tag I/O

src/library/scanner.rs:66-76 reads tags inside the write transaction, blocking the UI's playlist saves for the whole scan.

Fix: read tags outside, commit in short batches.
