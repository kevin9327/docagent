# DocAgent #1 loop state

Last cycle: 43 (2026-09-06) — six exclusive lanes in parallel

## Public GitHub page

https://github.com/kevin9327/docagent

## Cycle 43 lanes

1. **landing** — README first screen: deterministic convert · capsule · GPU-free. Honest matrix vs the ten.
2. **pdf** — `InlineObject::Image` layouts, paints, PDF/A XObject, raster blit.
3. **docx** — OOXML `a:blip` + `word/media`.
4. **odt** — `draw:image` + `Pictures/`.
5. **mdhtml** — `![alt](…)` and `<img>` data URIs.
6. **hangul** — Quote / CodeBlock / HorizontalLine on HWP5+HWPX+HML+HWP3.

## Next

- Put a real image in `examples/letter.md` and refresh GitHub convert shots.
- Hangul images / tasks.
- Keep visual density vs MinerU/Docling.

## Rule

Orchestrator fans out 6 lanes (LANES.md). One commit after clippy+tests. No overlapping files.
