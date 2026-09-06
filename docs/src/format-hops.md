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
[What ships](capabilities.md)), not this hop count.

A PNG or JPEG in Markdown, HTML, DOCX, or ODT becomes a PDF/A XObject. The demo
letter paints the teal harbor mark under the H2. That is picture in, PDF/A out —
not a hop count, and not OCR.

HTML `read` recovers `<img src="data:…">`; that is not a hop count and not an
HTML5 engine. Hangul images do not hop.
