# Inter-format file hops

Requirement: at least **100** lossless hops among formats DocAgent authors and
consumes.

Implementation: `crates/docagent-api/tests/format_hops.rs`

| Item | Value |
| --- | --- |
| Documents | 120 |
| Hops per document | 6 (`IR→DOCX→Markdown→ODT→HWP5→HWPX→HML`) |
| Total hops | 720 |
| Check | every non-blank `plain_text()` line of the source IR is present in the destination IR |
| Drivers | shipped `docagent-docx/md/odt/hwp5/hwpx/hml::{read,write}` |

```
cargo test -p docagent-api --test format_hops
```

Documents with `i % 3 == 0` also carry a table so the table path takes the same hops.
Global interchange (DOCX, Markdown, ODT) runs first; Hangul codecs follow on the same IR.

Hops prove **plain text** presence. Lists, links, strike, quotes, code, thematic
breaks, tasks, highlight, super/sub, table headers, and PNG / JPEG images
(Markdown / HTML / PDF/A / DOCX / ODT) are codec-level round-trips (see
[What ships](capabilities.md)), not this hop count. PDF float and cell XObjects,
PDF page header/footer text, DOCX `wp:wrapSquare` / `behindDoc` / in-front,
ODT nested `text:list`, cell `draw:image`, and Tight wrap (`Gtight`)
are also codec-level, not hop counts.
HTML `read` recovers `<img src="data:…">`, `<ul>`/`<ol>`/`<ul class="tasks">`,
`<th>`/`<thead>`, `<blockquote>`, `<pre>`, `<a href>`, and `<mark>`; that is not a hop count and
not an HTML5 engine.
Hangul Quote / CodeBlock / HorizontalLine / Task hop as para styles; highlight hops as CHAR_SHAPE shade.
Hangul table headers hop on HWP 5 / HWPX / HML (not HWP 3). Hangul Bullet / Number lists hop as para styles. Hangul images do not.
