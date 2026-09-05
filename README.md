# DocAgent

Document-only runtime for agents. No GUI, no spreadsheet, no slides.

Agents open HWP / HWPX / HWP 3 / HML / DOCX, work through one integer-HWPUNIT IR and one layout engine, and emit PDF/A plus semantic HTML. Every Command records input, plan, and output SHA-256 hashes.

Repository: https://github.com/kevin9327/docagent

```
cargo test --workspace
cargo run -p docagent-cli -- convert path/to/file.hwp --pdf out.pdf --html out.html --ir out.json --capsule out.capsule.json
cargo run -p docagent-conformance -- --require-win
```

CLI binary: `docagent`. Daemon: `docagentd`. See `AGENTS.md` and `docs/`.
