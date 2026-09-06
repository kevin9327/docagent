# Community codec guide

Markdown (`docagent-md`) and OpenDocument Text (`docagent-odt`) ship as first-class
codecs next to DOCX, HTML, and PDF/A. For RTF or another format:

1. Copy `crates/docagent-codec-template` and rename it (for example `docagent-rtf`).
2. Depend only on `docagent-model` and a format parser crate. Nothing at
   `docagent-layout` or below.
3. The contract is `sniff` / `read` / `write`. Lock round-trip text and tables with
   `proptest`. Map `CharStyle` (bold, italic, underline, strike, code, highlight,
   super/sub), `InlineObject::Hyperlink`, and `InlineObject::Image` (`image/png`,
   `image/jpeg`) when the format has those primitives. Picture in, PDF/A out is
   the product: a PNG or JPEG must survive into the IR so PDF/A can paint an
   XObject. Do not invent OCR.
4. Do not put a format identifier on the IR. Format-specific values become explicit
   `Option` fields.
5. Promote the crate to a workspace member only after parse-loss 0 on the
   `docagent-conformance` scoreboard.

RTF still uses the template crate only.

The GitHub first screen sells the agent runtime: 200px hero, unique-yes `<details>`
(C/Q/E · capsule SHA-256 ×3 · `prove` — none of the locked ten), convert trio
(teal mark under the H2), capsule SHA-256 ×3 lockstep with
[`letter-capsule.json`](../assets/letter-capsule.json)
(`input` `b0f44237…`, `plan` `4b7360eb…`, `output` `b468b37f…`)
**above** the vs-the-ten matrix, then `Command::Convert` / `prove` Quickstart
**below** that matrix.

```bash
cargo run -p docagent-cli -- convert examples/letter.md \
  --pdf out.pdf --html out.html --odt out.odt --docx out.docx --capsule cap.json
cargo run -p docagent-cli -- prove examples/letter.md
```

A new codec earns a row on [What ships](capabilities.md) only when those
primitives round-trip. CLI, daemon, MCP, and WIT stay thin adapters over the same
`Engine` (`Command` / `Query` / `Event`). Hangul images stay **—** until SCORE
names them. `prove` writes a separate JSON; those plan/output hashes stay
**distinct** (`928b4745…` / `2607bb6f…`) from the convert capsule — do not copy
them onto `letter-capsule.json`. Print-CSS tightness vs WeasyPrint is not a
codec-guide claim.
