# Docs drift: README version pin and docs/design.md

README's install snippet downloads `v=0.2.1` while the latest tag is `v0.3.1` (abs Y20).
`docs/design.md` still describes the v1 plan in the sections that state contracts: the config
key list, the engine `Command`/`Event` enums, the threading table, module layout (abs Y21).
Smaller doc items from the abstraction note's blue list belong here too (Files table, settings
paragraphs, the untagged 0.1.4 CHANGELOG entry, misplaced doc comments).

- Where: `README.md:36,48,137,146,155`, `docs/design.md:5,16,51,57-58,72,84,95-118,156-170,189`,
  `CHANGELOG.md:96`, `src/mpris.rs:107`, `src/audio/spectrum.rs:103`
- Done when: the README `v=` equals the latest released tag and the release check greps it; the
  config key list, the engine block and the threading table in `docs/design.md` match the code.
