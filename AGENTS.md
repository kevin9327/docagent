# DocAgent agent instructions

This is the only agent instruction file in the engine repository.

## Product

DocAgent is a document-only deterministic runtime for LLM agents, RPA, and backends.
It reads and writes flow documents. Humans receive PDF/HWP/HWPX/DOCX. There is no GUI, cursor, undo, spreadsheet, presentation, or hwpctl layer.

## Crate graph

Depend downward only. Codec crates (`docagent-hwp5`, `docagent-hwpx`, `docagent-hwp3`,
`docagent-hml`, `docagent-docx`) must not import each other or anything at
`docagent-layout` and below. `docagent-model` has no format identifier.

## Invariants CI enforces

1. No issue numbers in file, test, or constant names.
2. Layout/paint/raster/pdf do not know source formats.
3. One layout engine. Replacing it means deleting the old one in the same PR.
4. Paint backends: CPU (`tiny-skia`) plus optional GPU (`vello`).
5. Clippy `allow` ≤ 3 per crate, each with a rationale comment. No `dead_code` allow.
6. Corpus, Hangul PDFs, and oracles stay out of this repository.
7. This file is the only agent guide.
8. No PQ crypto, RAG, or settlement implementations. Signatures are Ed25519.
9. No spreadsheet or presentation models in the IR.

## Layout

All layout coordinates are HWPUNIT `i32` (1/7200 inch). Accumulators are `i64`.
`f32`/`f64` are forbidden in `docagent-layout`. LineSeg is a `LayoutHint` oracle
for the scoreboard; layout must not consume it.

## Commands

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo run -p docagent-lint`
- `cargo run -p docagent-cli -- convert <file> --pdf out.pdf --html out.html --ir out.json`
