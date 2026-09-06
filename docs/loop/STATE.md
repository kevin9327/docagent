# DocAgent #1 loop state

Last cycle: 48 (2026-09-06) — six exclusive lanes in parallel

## Public GitHub page

https://github.com/kevin9327/docagent

## Cycle 48 lanes

1. **landing** — Honest matrix vs the locked ten. Hangul images still **—**. No OCR or OmniDocBench numbers.
2. **pdf** — Section footer embeds in PDF/A; header-band `InlineObject::Image` layouts and paints as XObject.
3. **docx** — `WrapMode::Tight` round-trip (`wp:wrapTight`).
4. **odt** — `WrapMode::Behind` round-trip (`Gbehind`).
5. **mdhtml** — HTML `read` round-trips `<s>`/`<u>`/`<sup>`/`<sub>` and `<header>`/`<footer>`.
6. **hangul** — Nested Bullet/Number `NumberingRef.level` on HWP5+HWPX+HML+HWP3.

## Next

- Put a real image in `examples/letter.md` and refresh GitHub convert shots.
- Hangul images.
- Keep visual density vs MinerU/Docling.

## Rule

Orchestrator fans out 6 lanes (LANES.md). One commit after clippy+tests. No overlapping files.
