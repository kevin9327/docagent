# DocAgent #1 loop state

Last cycle: 45 (2026-09-06) — six exclusive lanes in parallel

## Public GitHub page

https://github.com/kevin9327/docagent

## Cycle 45 lanes

1. **landing** — Honest matrix vs the locked ten. Hangul highlight is **yes** (CHAR_SHAPE shade). Images still **—** on Hangul. No OCR or OmniDocBench numbers.
2. **pdf** — `Block::Float(Float::Image)` layouts and paints as PDF/A XObject; oversized floats clamp to content width.
3. **docx** — `WrapMode::Square` round-trip (`wp:wrapSquare`).
4. **odt** — `draw:image` inside table cells; `svg:desc` alt fallback.
5. **mdhtml** — HTML `read` round-trips `<blockquote>`, `<pre>`, `<a href>`, `<mark>`.
6. **hangul** — CHAR_SHAPE shade → `CharStyle.highlight` on HWP5+HWPX+HML+HWP3.

## Next

- Put a real image in `examples/letter.md` and refresh GitHub convert shots.
- Hangul images / first-class lists / table headers.
- Keep visual density vs MinerU/Docling.

## Rule

Orchestrator fans out 6 lanes (LANES.md). One commit after clippy+tests. No overlapping files.
