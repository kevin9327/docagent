# DocAgent #1 loop state

Last cycle: 13 (2026-09-06) — PDF heading-rule fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Layout paints heading rules as non-table `FillRect`s (H1 full-width 2pt `#134e4a`, H2 text-width 1pt `#0d9488`).
- Table cell fills stay on `rects`; heading rules go on `strokes` so `table_row_heights` is unchanged.
- HTML matches with `border-bottom` on `h1`/`h2`.
- Regenerated [letter.pdf](../assets/letter.pdf), [letter.html](../assets/letter.html), capsule, prove JSON, and README screenshots.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `5f33b7f2389a3c4a4f53b638808a8e958305cc8f8a77b976d03f3d41f3d4a85f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `0fc6d675b4c5a6c773d3da390d7c7e206d0369e232db662fa293c3c415908ccf`

## Next

1. Heading size in ODT text-properties, or HTML table-cell emphasis.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
