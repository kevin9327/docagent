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

- Demo keep `letter.*` / `output-*.png` in lockstep with the denser PDF page after `e93747f`.
- Next gap: PDF page looking as dense as WeasyPrint/MinerU.
- Hangul images still **—**; **stop deepening Hangul**.

## Rule

Orchestrator fans out 6 lanes (LANES.md). One commit after clippy+tests. No overlapping files.

## Cycle 49 landing (2026-09-06) — historical

First screen sold **picture in, PDF/A out**. Hero / pipeline / ships dropped wrap-mode and Hangul trivia. Images row: PNG+JPEG **yes** on MD/HTML/PDF/A/DOCX/ODT, Hangul **—**. Demo letter landed `![Harbor mark](docs/assets/mark.png)`. Superseded: those convert shots now paint the teal mark; demo-image gap is **closed** (see Cycle 53). No OCR claims.

## Cycle 50 landing (2026-09-06) — historical

Shallow score vs the locked ten. `examples/letter.md` has `![Harbor mark](docs/assets/mark.png)`. Superseded: `output-pdf.png`, `output-html.png`, and `output-trio.png` now show the harbor mark under the H2; GitHub-image gap is **closed** (see Cycle 53). No new IR. No OCR. Hangul still **stop deepening**.

## Cycle 51 landing (2026-09-06)

GitHub first screen now uses the refreshed convert shots (`output-trio.png`, `output-pdf.png`, `output-html.png`) that show the teal harbor mark under the H2. Dropped every stale / no-picture caption. Capsule hashes on README match `letter-capsule.json`. Capsule / determinism / GPU-free kept. Demo-image gap **closed**. Next gap: PDF page looking as dense as WeasyPrint/MinerU — still no invented numbers. No OCR. No wrap. Hangul still **stop deepening**.

## Cycle 52 (2026-09-06)

Committed finished lanes: DOCX `a:blip` + media PNG from `letter.md`; ODT `draw:image` + Pictures/ PNG; demo capsule/prove/`letter.pdf`/`output-*.png` after the 24pt mark scale. Next: landing lockstep README hashes with capsule; PDF page density vs WeasyPrint/MinerU; demo keep assets in lockstep. docx/odt/mdhtml: do not invent work. No OCR. Hangul still **stop deepening**.

## Cycle 53 landing (2026-09-06)

First screen uses the harbor-mark convert shots (`output-trio.png`, `output-pdf.png`, `output-html.png`) plus `mark.png`. Dropped stale / no-picture captions and the concept-art JPGs (`convert-story.jpg`, `capsule-page.jpg`) that are not engine output. README capsule hashes lockstep with `letter-capsule.json` (`output_hash` `f09d223f56e8adec474d9c24c3f0b2667f72104b1f67465f5e63c6c997a19ddf`). Capsule / determinism / GPU-free kept. Demo-image gap **closed**. Cycles 49–50 “text-only / gap open” captions are historical. Next gap: PDF page density vs WeasyPrint/MinerU. No OCR. No OmniDocBench numbers. Hangul still **stop deepening**.

## Cycle 54 (2026-09-06)

Committed PDF letter-page density (type, spacing, table) vs WeasyPrint/MinerU plus landing captions/hashes. Demo must refresh `letter.*` / `output-*.png` so GitHub matches the denser page. docx/odt/mdhtml: do not invent work. No OCR. Hangul still **stop deepening**.

## Cycle 55 landing (2026-09-06)

First screen uses the harbor-mark convert shots (`output-trio.png`, `output-pdf.png`, `output-html.png`) plus `mark.png`. Captions already show the teal mark under the H2; no text-only / gap-open language; concept-art JPGs stay unused. After density commit `e93747f`, README and `docs/src` hashes lockstep with `letter-capsule.json` (`output_hash` `e38bb197cbc8a9379834e71c2a4827d5d8a5cc70af67ef2a68501fceccce916a`; input and plan unchanged). Capsule / determinism / GPU-free kept. Demo-image gap **closed**. No OCR. No OmniDocBench numbers. Hangul still **stop deepening**.

## Cycle 56 (2026-09-06)

Committed denser-page `letter.pdf` / `output-*.png` plus README hash lockstep (`output_hash` `e38bb197…`). Next gap still PDF page density vs WeasyPrint/MinerU — no invented numbers. docx/odt/mdhtml: do not invent work. Hangul still **stop deepening**.

## Cycle 57 landing (2026-09-06)

Master `5d51779` already locksteps README / `docs/src` / `competitors.json` with `letter-capsule.json` (`output_hash` `e38bb197cbc8a9379834e71c2a4827d5d8a5cc70af67ef2a68501fceccce916a`). First screen is harbor-mark convert shots (`output-trio.png`, `output-pdf.png`, `output-html.png`) plus `mark.png`; captions already say teal under H2; concept-art JPGs stay unused. No README churn. Capsule / determinism / GPU-free kept. Demo-image gap **closed**. No OCR. No OmniDocBench numbers. Hangul still **stop deepening**.

## Cycle 58 score (2026-09-06)

Hourly score rewritten. Demo-image gap **closed**. Remaining gap: WeasyPrint table/paragraph tightness (heading density already locked). prove JSON still looks stale vs capsule. docx/odt/mdhtml: do not invent work. Hangul still **stop deepening**.
