# DocAgent #1 loop state

Last cycle: 3 (2026-09-06) — PDF/A table grid + refreshed convert screenshots

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- PDF/A now paints table cell fills and 1pt grid strokes from the display list (HTML already had borders; the README PDF shot was tab-separated columns).
- Header row uses the same mint fill as HTML `<th>`.
- Markdown tables set border width to 1pt (`HU_PER_POINT`). Stroke rects live on `PageFrag.strokes` so Hangul `table_row_heights` is unchanged.
- Reconverted `examples/letter.md` and refreshed `output-pdf.png` / `output-html.png` / `output-capsule.png` / `output-trio.png`.

## Next

1. Byte-identical rerun proof on the README (two-run hash equality fixture from `convert_is_byte_identical_across_two_runs`). Still no invented OmniDocBench numbers.
2. Then the next layout gap that still loses on a real letter: list markers, or PDF drawing of non-table fills.
3. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
