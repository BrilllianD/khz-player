# Slice 5: Library entry for downloaded files

Part C slice 5 of epic 148. Outline: /home/bronnikov/.claude/plans/khz-partc/outline.md (section 'Slice 5'); constraints: outline-constraints.md next to it.

Boundary: `src/library`
Depends on: Slice 1
1. `upsert_one`, `source_url`, `has_video`, W = 3 (files 3: `src/library/db.rs`,
   `src/library/tags.rs`, `src/audio/decoder.rs` (test only) → 1; +2 new interface). Scope:
   one-track upsert, source URL from tags, video-track detection that ignores cover streams,
   tests on ffmpeg fixtures. Consumes `fixtures::ffmpeg_fixture`.
ΣW = 3. Produces: `db::upsert_one`, `tags::source_url`, `tags::has_video`.
Must verify: a cover embedded by ffmpeg as an `attached_pic` stream in `.mp4`, and a picture in
`.opus`, are not reported as video; an `.mp4` with H.264 and AAC plays its audio through the
engine today (a decoder test: `Decoder::open` picks the audio track); how long `has_video`
takes on such a file (it runs at play time; if it is over ~20 ms, record that in the plan and
the routing designer caches it per path, still no schema change).
