# DocAgent #1 loop state

Last cycle: 46 (2026-09-06) — six exclusive lanes in parallel

## Public GitHub page

https://github.com/kevin9327/docagent

## Cycle 46 lanes

1. **landing** — Honest matrix vs the locked ten. Hangul table headers **yes** on HWP 5 / HWPX / HML (not HWP 3). Still **—**: Hangul images, first-class lists. No OCR or OmniDocBench numbers.
2. **pdf** — Table-cell `InlineObject::Image` layouts, paints, and embeds as PDF/A XObject; clamped to cell width.
3. **docx** — `WrapMode::Behind` round-trip (`wp:wrapNone` + behindDoc).
4. **odt** — Nested `text:list` inside `text:list-item` (level 0 then 1).
5. **mdhtml** — HTML `read` round-trips `<ul>`, `<ol>`, `<ul class="tasks">`.
6. **hangul** — Table header rows on HWP5+HWPX+HML; HWP 3 documents no header primitive.

## Next

- Put a real image in `examples/letter.md` and refresh GitHub convert shots.
- Hangul images / first-class lists.
- Keep visual density vs MinerU/Docling.

## Rule

Orchestrator fans out 6 lanes (LANES.md). One commit after clippy+tests. No overlapping files.
