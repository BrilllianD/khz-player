## 2026-10-14T15:12:02Z — Bronnikov Aleksandr

on it

## 2026-10-14T15:12:26Z — Bronnikov Aleksandr

39ac798: H1 fixed (fixed-height status row, explicit tree height); needs manual check of GUI

## 2026-10-14T15:45:36Z — Bronnikov Aleksandr

Real cause: Jump to file (J) ScrollArea used auto_shrink [false,true], so centred Modal changed height with hit count. Fixed in c48db4f (min_scrolled_height 360, auto_shrink false). Earlier library panel fix was unrelated.

