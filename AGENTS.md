# DocHWP agent instructions

This is the only agent instruction file in the engine repository.

## Product

DocHWP is a deterministic document runtime for LLM agents, RPA, and backends.
Humans receive PDF/HWP/HWPX/DOCX. There is no GUI, cursor, undo, or hwpctl layer.

## Crate graph

Depend downward only. Codec crates (`dochwp-hwp5`, `dochwp-hwpx`, `dochwp-hwp3`,
`dochwp-hml`, `dochwp-docx`) must not import each other or anything at
`dochwp-layout` and below. `dochwp-model` has no format identifier.

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
`f32`/`f64` are forbidden in `dochwp-layout`. LineSeg is a `LayoutHint` oracle
for the scoreboard; layout must not consume it.

## Commands

- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo run -p dochwp-lint`
- `cargo run -p dochwp-cli -- convert <file> --pdf out.pdf --html out.html --ir out.json`
