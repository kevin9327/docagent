# DocAgent #1 loop state

Last cycle: 27 (2026-09-06) — GFM task-list checkbox fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- IR adds `NumberFormat::Task` (`start == Some(1)` means checked).
- Layout paints a hollow teal box; checked items get an inner fill. No `•` glyph.
- Markdown reads/writes `- [x]` / `- [ ]`. HTML emits disabled checkboxes in `ul.tasks`. DOCX/ODT keep `[x]` / `[ ]` prefixes on roundtrip.
- Fixture tasks: dock clear and hashes checked; "hoping" unchecked. Nested bay stays a bullet.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `d8604c5c02966b38f7516918e0ff33f59640ac304895561645cbc6be80de1b52`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `17aa085cdbb55b6739fb8485fba68adc055917eb19fb7e493b6be12a017218af`

Plan hash matches cycle 21–26 (plan is command + targets + fonts, not the paint tree). Input and output hashes changed with the fixture and task boxes.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks / tasks, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
