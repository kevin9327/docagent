# DocAgent #1 loop state

Last cycle: 7 (2026-09-06) — nested lists on the sample letter

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Markdown nested lists (`  -` / `   -`) set `NumberingRef.level` and hanging indent.
- HTML emits nested `<ul>`/`<ol>` inside the parent `<li>`.
- Layout now places lines at `origin_x + line.x` so list indent is not discarded.
- Sample letter shows a nested dock note and nested PDF/A·HTML under Convert. Screenshots and prove hashes refreshed.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `a328a883d9d711e046d86839b69b19684c3187f7916d2082f7318671a0791988`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `ec44ccc0879c16189ab9d9d3d335439a3f082a044126fb83473e8ed33a42e088`

## Next

1. Inline emphasis (`**bold**` / `_italic_`) in markdown → HTML `<strong>`/`<em>` (PDF needs a bold face).
2. Or PDF drawing of non-table fills beyond the grid.
3. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
