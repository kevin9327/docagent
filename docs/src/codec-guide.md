# Community codec guide

Markdown (`docagent-md`) and OpenDocument Text (`docagent-odt`) ship as first-class
codecs next to DOCX. For RTF or another format:

1. Copy `crates/docagent-codec-template` and rename it (for example `docagent-rtf`).
2. Depend only on `docagent-model` and a format parser crate. Nothing at
   `docagent-layout` or below.
3. The contract is `sniff` / `read` / `write`. Lock round-trip text and tables with
   `proptest`. Map `CharStyle` (bold, italic, underline, strike, code, highlight,
   super/sub), `InlineObject::Hyperlink`, and `InlineObject::Image` (`image/png`,
   `image/jpeg`) when the format has those primitives. If the format has wrapping
   or table cells, map `WrapMode::Square` and cell-level images the way DOCX and
   ODT already do.
4. Do not put a format identifier on the IR. Format-specific values become explicit
   `Option` fields.
5. Promote the crate to a workspace member only after parse-loss 0 on the
   `docagent-conformance` scoreboard.

RTF still uses the template crate only.
