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

## Cycle 49 landing (2026-09-06)

First screen now sells **picture in, PDF/A out**. Hero / pipeline / ships drop wrap-mode and Hangul trivia. Images row: PNG+JPEG **yes** on MD/HTML/PDF/A/DOCX/ODT, Hangul **—**. Demo letter landed `![Harbor mark](docs/assets/mark.png)`; README points at that line and at `docs/assets/mark.png`. `output-*.png` were not rewritten by landing — captions stay honest if those frames are still text-only. No OCR claims.

## Cycle 50 landing (2026-09-06)

Shallow score vs the locked ten. `examples/letter.md` has `![Harbor mark](docs/assets/mark.png)`. `output-pdf.png`, `output-html.png`, and `output-trio.png` are still text-only — no Harbor mark painted. Captions on README / intro / capabilities stay honest: GitHub-image gap **open**. Next gap is not WeasyPrint until those shots show the mark. No new IR. No OCR. Hangul still **stop deepening**.

## Cycle 51 landing (2026-09-06)

GitHub first screen now uses the refreshed convert shots (`output-trio.png`, `output-pdf.png`, `output-html.png`) that show the teal harbor mark under the H2. Dropped every stale / no-picture caption. Capsule hashes on README match `letter-capsule.json`. Capsule / determinism / GPU-free kept. Demo-image gap **closed**. Next gap: PDF page looking as dense as WeasyPrint/MinerU — still no invented numbers. No OCR. No wrap. Hangul still **stop deepening**.
