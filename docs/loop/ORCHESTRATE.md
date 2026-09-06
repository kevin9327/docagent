# DocAgent #1 loop — 6-way fan-out

You are the orchestrator. Do not implement codecs yourself unless a lane fails.

Repo: `C:\Users\swsz9\docagent`
Read `docs/loop/LANES.md` and `docs/loop/STATE.md` first.

## Fan-out (required)

Spawn **exactly 6** `general-purpose` subagents in **one** parallel wave (`isolation` none).
Each prompt names one lane from LANES.md, exclusive write paths, and one concrete gap vs the locked ten (MinerU, Docling, Pandoc, Marker, Unstructured, PyMuPDF, WeasyPrint, ONLYOFFICE, python-docx, LibreOffice).

Rules for children:
- Work only in `C:\Users\swsz9\docagent`.
- Do not commit or push.
- Do not edit `crates/docagent-model` (IR already has `ImageData`, `Hyperlink`, quote, code_block, Task, Thematic).
- Do not edit `examples/letter.md`.
- `cargo test -p <their-crate> --offline` must pass.
- `cargo clippy -p <their-crate> --all-targets -- -D warnings` must pass.
- If blocked, write why and stop. Do not steal another lane.

Default lane tasks when STATE says Hangul links are done:

1. **landing** — Honest README matrix vs the ten; keep visual density. Do not invent OCR scores.
2. **pdf** — Layout + PDF/A paint `InlineObject::Image` (width/height in HWPUNIT).
3. **docx** — Read/write `ImageData` through OOXML drawing/blip.
4. **odt** — Read/write `ImageData` through `draw:image`.
5. **mdhtml** — Markdown `![alt](…)` and HTML `<img>`.
6. **hangul** — Quotes / code_block / thematic breaks on HWP5+HWPX+HML+HWP3 (not images unless trivial).

If a lane’s feature already ships, pick the next gap **inside that lane’s files only**.

## After all six return

1. `cargo clippy --workspace --all-targets -- -D warnings`
2. `cargo test --workspace --offline`
3. If green: one commit covering all lane diffs, `git push origin master`, `gh run list --limit 1`. Fix red CI before the cycle ends.
4. Update `docs/loop/STATE.md` with cycle number and what each lane shipped.
5. Never spawn a second wave in the same firing.
