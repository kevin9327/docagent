# DocAgent #1 loop state

Last cycle: 15 (2026-09-06) — HTML/MD table-cell emphasis

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- HTML table cells emit `<strong>`/`<em>` from IR runs instead of flattening to plain text.
- Markdown pipe tables parse `**bold**` / `_italic_` in cells; the header row is bold.
- Regenerated letter PDF/A, HTML, DOCX, ODT, capsule, prove JSON, and README screenshots. Input hash unchanged.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `5f33b7f2389a3c4a4f53b638808a8e958305cc8f8a77b976d03f3d41f3d4a85f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `c9eec11811ab1d07c37395732088c6d1f8c46f319a641572cf1af05ea230ee4c`

## Next

1. Markdown table separator row on write, or list-marker fills in PDF.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
