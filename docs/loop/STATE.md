# DocAgent #1 loop state

Last cycle: 44 (2026-09-06) — six exclusive lanes in parallel

## Public GitHub page

https://github.com/kevin9327/docagent

## Cycle 44 lanes

1. **landing** — Honest matrix vs the locked ten: Hangul Quote / CodeBlock / HorizontalLine / Task are yes; images yes on MD/HTML/PDF/DOCX/ODT. No OCR or OmniDocBench numbers.
2. **pdf** — JPEG `InlineObject::Image` embeds as PDF/A XObject; oversized inline images clamp to content width (integer HU).
3. **docx** — JPEG `ImageData` round-trip through `word/media` + `[Content_Types].xml` `image/jpeg`.
4. **odt** — JPEG `draw:image` + `Pictures/*.jpg`; non-inline wrap as LibreOffice `style:wrap`.
5. **mdhtml** — HTML `read` recovers `<img src="data:…">`; Markdown `![alt](src "title")` parse.
6. **hangul** — Task style + `[x]` / `[ ]` on HWP5+HWPX+HML+HWP3.

## Next

- Put a real image in `examples/letter.md` and refresh GitHub convert shots.
- Hangul images / highlight / first-class lists.
- Keep visual density vs MinerU/Docling.

## Rule

Orchestrator fans out 6 lanes (LANES.md). One commit after clippy+tests. No overlapping files.
