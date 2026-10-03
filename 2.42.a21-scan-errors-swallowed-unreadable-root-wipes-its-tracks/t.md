# A21: scan errors swallowed; unreadable root wipes its tracks

Walk errors are ignored and delete_unseen removes every track under a root that is empty or unreadable, e.g. an unmounted drive (src/library/scanner.rs:53,82; app root filter src/app.rs:617).

Fix: report walk errors; skip delete_unseen for roots that could not be read or yielded zero files.
