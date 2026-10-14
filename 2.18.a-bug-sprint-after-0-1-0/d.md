## 2026-10-05T14:13:32Z — Bronnikov Aleksandr

status after 0.1.3

## 2026-10-13T17:16:01Z — Bronnikov Aleksandr

Status 2026-10-13: 0.2.2 released (tag v0.2.2, 7299410). Closed A18 39, A19 40, A20 41, A21 42, A22 43, A23 44, A24 45, A26 47, E5 72, E10 77, E11 78. Tests 87 -> 117. Review minors not fixed: nested roots (/m prunable while /m/sub unmounted deletes /m/sub tracks); M3U '#' line after #EXTINF taken as path even for '# comment' / '#KODIPROP:'; file:///C:/... not mapped by drive fallback; drive path ending in separator yields base dir. Unverified: MPRIS Position may show old track position for one frame after load. Next: session 2 (A27, A28, A29, B1 closes A25, D5, D2 CI), release 0.2.3.

## 2026-10-14T15:36:51Z — Bronnikov Aleksandr

Closed in 0.3.0 (e3a832c): A25/B1 single instance bfeca54, A27 drop at end d77e673, A28 off-thread add b38ec8f, A29 EQ AUTO + UI 404d276. GUI checks still open for the user: drag below last row, large-folder add, panel widths, forward into running window.

