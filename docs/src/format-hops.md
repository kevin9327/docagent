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

**North star (not a hop count):** drop `examples/letter.md` that contains
`![Harbor mark](docs/assets/mark.png)`, and get PDF/A + HTML + Markdown + DOCX +
ODT whose bytes match the next run, with three hashes in
[`letter-capsule.json`](../assets/letter-capsule.json)
(`input` `b0f4423728db4238779b3bf6cbd238d4d0d637f733c078322be6ff23398bbcdf`,
`plan` `4b7360eb1b7a9bec20cc35d5c79f2156c22d2712ab62ededfb60e328b8f16d3a`,
`output` `b468b37f1c3d8da7e4aac7e1a8417b46c29a57863817f9df16e8d4a6cc89f0f8`).
The picture is a PDF/A XObject. Not OCR.

A PNG or JPEG in Markdown, HTML, DOCX, or ODT becomes a PDF/A XObject. The demo
letter paints the teal harbor mark under the H2. The GitHub first screen is a
200px hero, unique-yes `<details>`, that convert trio, and the three capsule
hashes **above** the vs-the-ten matrix; Quickstart (`convert` + `prove` +
`Engine`) is **below** the matrix. The demo-image gap is **closed**. That is
picture in, PDF/A out — not a hop count, and not OCR. `prove` is a separate
JSON from the convert capsule; those plan/output hashes stay **distinct**
(`928b4745…` / `2607bb6f…`) — do not copy them onto the capsule table.
Print-CSS tightness vs WeasyPrint is not this page and is **not** claimed closed.

```bash
cargo run -p docagent-cli -- convert examples/letter.md \
  --pdf out.pdf --html out.html --odt out.odt --docx out.docx --capsule cap.json
cargo run -p docagent-cli -- prove examples/letter.md
```

HTML `read` recovers `<img src="data:…">`; that is not a hop count and not an
HTML5 engine. Hangul images do not hop.
