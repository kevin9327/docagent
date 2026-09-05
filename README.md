# DocAgent

Document-only runtime for agents. Not an office suite. Not a Hangul-only tool.

Agents convert **DOCX, ODT, Markdown, HTML, and PDF/A** through one integer layout
engine (1/7200 inch). Regional office formats (**HWP / HWPX / HML**) use the same
IR. Every Command records input, plan, and output SHA-256 hashes.

https://github.com/kevin9327/docagent

```
cargo test --workspace
cargo run -p docagent-cli -- convert letter.docx --pdf out.pdf --html out.html --md out.md --odt out.odt --capsule cap.json
```

Binary: `docagent`. Daemon: `docagentd`. See `AGENTS.md` and `docs/`.
