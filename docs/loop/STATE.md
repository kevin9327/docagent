# DocAgent #1 loop state

Last cycle: 16 (2026-09-06) — PDF list-marker fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Bullet lists paint a square `FillRect` (`#134e4a`) instead of a `•` glyph. Decimal lists still emit `1. `.
- Marker fills sit on `strokes`, so `table_row_heights` is unchanged.
- Regenerated letter.pdf, capsule, prove JSON, and README screenshots. Input hash unchanged.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `5f33b7f2389a3c4a4f53b638808a8e958305cc8f8a77b976d03f3d41f3d4a85f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `e858e36d71264df5c43cf35505c7f0327551acdd2e8dfa02c3989d89785856ed`

## Next

1. Markdown table separator row on write.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
