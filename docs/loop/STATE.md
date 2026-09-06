# DocAgent #1 loop state

Last cycle: 8 (2026-09-06) — markdown/HTML inline emphasis

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Markdown `**bold**`, `*italic*`, `_italic_` become IR `CharStyle` runs (asterisks are not left in the text).
- HTML emits `<strong>` / `<em>`. Sample letter uses **GPU-free** and _byte-for-byte_.
- PDF still uses the single Regular face; emphasis is in the IR and HTML screenshot. Screenshots and prove hashes refreshed.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `5f33b7f2389a3c4a4f53b638808a8e958305cc8f8a77b976d03f3d41f3d4a85f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `910fabe0392779c89fdc1e0920e8d0a36f659cf89dd349364bca05e2d21299b8`

## Next

1. PDF emphasis: bundle a bold face or synthetic bold on `CharStyle.bold` spans.
2. Or PDF drawing of non-table fills beyond the grid.
3. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
