# DocAgent #1 loop state

Last cycle: 21 (2026-09-06) — PDF URI /Link annotations

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Layout carries `LineFrag.href`. Paint emits `Op::Link`.
- PDF/A writes clickable URI `/Link` annotations (krilla `LinkAction`) over the hyperlink hit box.
- Fixture last line is still `[DocAgent](https://github.com/kevin9327/docagent)` — the underline fill stays; the PDF is now a real link.
- Regenerated letter PDF/A, HTML, capsule, prove JSON, and README screenshots. DOCX/ODT/MD unchanged.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `64f94fb0ffac49588de2ff9153943bd020205d1a846c06731a9e60d63efea29f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `cc198ebbf428700151bd4c84d16b44c87fbaa5d30a20204e0c95d60b90d8722f`

Input and plan hashes match cycle 20. Output hash changed with the annotation objects.

## Next

1. Hangul codec hyperlinks, or strikethrough, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
