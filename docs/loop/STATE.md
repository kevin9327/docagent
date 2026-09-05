# DocAgent #1 loop state

Last cycle: 6 (2026-09-06) — ordered lists (`1.`) on the sample letter

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Markdown `1. item` keeps `NumberFormat::Decimal` and `start` on the IR.
- Layout prefixes `1. ` / `2. ` with hanging indent. HTML emits `<ol><li>`.
- `examples/letter.md` now has an agent-replay numbered list. Convert screenshots and prove hashes refreshed.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `e28e664baf239f721fd4965f699ef959a8c7d0ef4e84412768d39342db21cd6f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `2da54d85c18913bb9df41d6df7ac015f3a915969af796fc0aa81fc9ab7e90833`

## Next

1. Nested lists, or PDF drawing of non-table fills beyond the grid.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
