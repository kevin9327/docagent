# DocAgent #1 loop state

Last cycle: 5 (2026-09-06) — markdown/PDF/HTML list markers on the sample letter

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Markdown `- ` / `* ` keeps `NumberFormat::Bullet` on the IR instead of stripping the marker.
- Layout prefixes `• ` on the first line with hanging indent. HTML emits `<ul><li>`.
- `examples/letter.md` now has a three-item confirm list. Convert screenshots and prove hashes refreshed.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `c863f8c0aacec10b0665dcdf2893c909427f76f49c64db9167a9a6f68ab34c28`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `5164eb2e67d76ef690895002b8ec1451a6dcd1cb43f5838e1d621f707028feb0`

## Next

1. Ordered lists (`1.`) on the same numbering path.
2. Or PDF drawing of non-table fills beyond the grid.
3. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
