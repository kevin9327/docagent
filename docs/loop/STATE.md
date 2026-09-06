# DocAgent #1 loop state

Last cycle: 47 (2026-09-06) — six exclusive lanes in parallel

## Public GitHub page

https://github.com/kevin9327/docagent

## Cycle 47 lanes

1. **landing** — Honest matrix vs the locked ten. Hangul Bullet / Number lists **yes**. Still **—**: Hangul images. No OCR or OmniDocBench numbers.
2. **pdf** — `Section.header` / `footer` layout in margin bands; PDF/A embeds header text.
3. **docx** — `WrapMode::InFront` round-trip (`wp:wrapNone`, not behindDoc).
4. **odt** — `WrapMode::Tight` round-trip (`Gtight`).
5. **mdhtml** — HTML `read` round-trips `<th>` / `<thead>` table headers.
6. **hangul** — Bullet / Number para styles on HWP5+HWPX+HML+HWP3.

## Next

- Put a real image in `examples/letter.md` and refresh GitHub convert shots.
- Hangul images.
- Keep visual density vs MinerU/Docling.

## Rule

Orchestrator fans out 6 lanes (LANES.md). One commit after clippy+tests. No overlapping files.
