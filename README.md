# DocHWP

Deterministic flow-document runtime for agents. No GUI.

Repository: https://github.com/kevin9327/dochwp

HWP 5 / HWPX / HWP 3 / HML / DOCX go through one integer-HWPUNIT IR and one layout engine, then PDF/A and semantic HTML. Every Command records input, plan, and output SHA-256 hashes.

```
cargo test --workspace
cargo run -p dochwp-cli -- convert path/to/file.hwp --pdf out.pdf --html out.html --ir out.json --capsule out.capsule.json
cargo run -p dochwp-conformance -- --require-win
```

Sibling `rhwp` `devel` is the scoreboard baseline (not vendored). See `AGENTS.md` and `docs/`.
