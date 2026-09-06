# DocAgent #1 loop state

Last cycle: 25 (2026-09-06) — thematic-break rule fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- IR adds `BreakKind::Thematic`. Layout paints a full-width teal `FillRect` (80 HU).
- Markdown reads/writes `---`. HTML emits `<hr/>`. DOCX emits `w:pStyle HorizontalLine` plus a bottom `w:pBdr`. ODT emits `text:p` HorizontalLine with `fo:border-bottom`.
- Fixture inserts `---` after the hop table so the GitHub PDF/HTML screenshots show a real rule.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `382159e5d15d629c4aac774fa906542a42729ba3d23f1eb682da7dfe14fdd6f1`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `10b4d9226d4a6c441b7801bf7849515392ffc256e9bdc5363aa74f32d803736f`

Plan hash matches cycle 21–24 (plan is command + targets + fonts, not the paint tree). Input and output hashes changed with the fixture and rule fill.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
