# DocAgent #1 loop state

Last cycle: 11 (2026-09-06) — DOCX/ODT CharStyle

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- DOCX write emits per-run `w:rPr` (`w:b` / `w:i`); read restores `CharStyle`.
- ODT write emits `text:span` + automatic styles (`fo:font-weight` / `fo:font-style`); read restores them.
- `examples/letter.md` convert keeps **GPU-free** and _byte-for-byte_ in [letter.docx](../assets/letter.docx) and [letter.odt](../assets/letter.odt).
- README links those files next to PDF/A and HTML. Prove hashes unchanged (PDF/A + HTML only).

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `5f33b7f2389a3c4a4f53b638808a8e958305cc8f8a77b976d03f3d41f3d4a85f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `b83ef5d60c7c860a840c7ca1334516dfad44362e6811a5605f7a5586a78fb975`

## Next

1. PDF fills beyond table grids, or DOCX/ODT heading styles / table-cell emphasis.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
