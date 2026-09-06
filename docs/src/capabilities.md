# What ships

DocAgent is an **agent document runtime**. The GitHub page is scored against the
ten most-starred document repositories an agent actually meets (2026-09-06):
MinerU, Docling, Pandoc, Marker, Unstructured, PyMuPDF, WeasyPrint, ONLYOFFICE,
python-docx, LibreOffice.

This page is the honest matrix. It is not an OCR bake-off. There are no
OmniDocBench scores, no VLM accuracy claims, and no invented GPU numbers.

**North star:** drop `examples/letter.md` (or DOCX/ODT) that contains a PNG or
JPEG, and get PDF/A + HTML + Markdown + DOCX + ODT whose bytes match the next
run, with three hashes in the capsule. The picture is a PDF/A XObject. Not OCR.

First-screen job: **picture in, PDF/A out.** A PNG or JPEG in Markdown, HTML,
DOCX, or ODT becomes a PDF/A XObject. We embed the file. We do not OCR it.
The GitHub page leads with a 200px hero, a collapsed unique-yes `<details>`
(C/Q/E · capsule SHA-256 ×3 · `prove` — none of the locked ten), the harbor-mark
convert trio (teal under the H2), and the three capsule hashes **above** the
vs-the-ten matrix. Quickstart (`convert` + `prove` + `Engine`) is **below** that
matrix — MinerU/Docling visual density for an agent runtime, not an OCR bake-off.
The demo-image gap is **closed**. Capsule hashed. GPU-free. Print-CSS tightness
vs WeasyPrint is **not** claimed closed.

## Runtime axes

These are the axes DocAgent wins on. Blank cells mean the other project does
not ship that axis as a product feature. Tools are rows.

| Tool | Job | C/Q/E | Capsule | Same bytes | i32 layout | GPU-free | Hangul |
| --- | --- | :---: | :---: | :---: | :---: | :---: | :---: |
| **DocAgent** | Agent document runtime | **yes** | **SHA-256 ×3** | **yes** | **1/7200"** | **yes** | **yes** |
| MinerU 79k | PDF/Office → LLM markdown | | | | | | |
| Docling 66k | Gen-AI parse + export | | | | | optional | |
| Pandoc 46k | Universal markup convert | | | | | **yes** | |
| Marker 40k | PDF → markdown/JSON | | | | | | |
| Unstructured 15k | Document ETL for LLMs | | | | | | |
| PyMuPDF 11k | PDF extract/convert | | | | | **yes** | |
| WeasyPrint 10k | HTML/CSS → PDF | | | | | **yes** | |
| ONLYOFFICE 6.9k | Collaborative office suite | | | | | **yes** | |
| python-docx 5.7k | Word DOCX read/write | | | | | **yes** | |
| LibreOffice 4.3k | Headless office convert | | | | | **yes** | |

`prove examples/letter.md` is the merge gate: two converts, identical PDF/A + HTML
bytes, identical input / plan / output SHA-256. README hashes lockstep with
[`letter-capsule.json`](../assets/letter-capsule.json). `prove` writes a separate
[`letter-prove.json`](../assets/letter-prove.json); those plan/output hashes stay
**distinct** (`928b4745…` / `2607bb6f…`) from the convert capsule — do not copy
them onto the README table.

```bash
cargo run -p docagent-cli -- convert examples/letter.md \
  --pdf out.pdf --html out.html --odt out.odt --docx out.docx --capsule cap.json
cargo run -p docagent-cli -- prove examples/letter.md
```

Every Command writes a capsule. CLI, daemon, MCP, and WIT are thin adapters over
the same `Engine` (`Convert` | `Import` | `Export` | `LayoutDocument`).

## Flow marks on the IR

These marks round-trip through the named codec. A dash is a real gap, not a
teaser.

| Mark | Markdown | HTML | PDF/A | DOCX | ODT | Hangul |
| --- | :---: | :---: | :---: | :---: | :---: | :---: |
| Lists (ul/ol, nested) | **yes** | `read` ul/ol/tasks | painted markers | `w:numPr` | `text:list` | **yes** |
| Links | `[text](url)` | `<a href>` | URI `/Link` | `w:hyperlink` | `text:a` | **yes** |
| Strike | `~~text~~` | `<s>` | fill | `w:strike` | line-through | **yes** |
| Quotes | `>` | `<blockquote>` | quote bar | Quote | Quote | **yes** |
| Code spans / fences | `` ` `` / ` ``` ` | `<code>` / `<pre>` | fills | Code / CodeBlock | Tcode / CodeBlock | **yes** |
| Thematic break | `---` | `<hr/>` | rule fill | HorizontalLine | HorizontalLine | **yes** |
| Tasks | `- [x]` | checkbox | task box | `[x]` | `[x]` | **yes** |
| Highlight | `==mark==` | `<mark>` | fill | `w:highlight` | Tmark | **yes** |
| Superscript / subscript | `^ ^` / `~ ~` | `<sup>` / `<sub>` | baseline shift | `w:vertAlign` | Tsuper / Tsub | **yes** |
| Underline | `__text__` | `<u>` | fill | `w:u` | Tunder | **yes** |
| **Images (PNG, JPEG)** | `![alt](…)` | `<img>` | **XObject** | `a:blip` | `draw:image` | **—** |
| Table header row | GFM `\| --- \|` | `<th>` / `<thead>` | header fill | `w:tblHeader` | `THcell` | **yes** |

**Images are the first-class row.** PNG and JPEG round-trip on Markdown, HTML,
PDF/A, DOCX, and ODT. They stay **—** on Hangul. That dash is real.

`examples/letter.md` includes `![Harbor mark](docs/assets/mark.png)`. First-screen
convert shot is [`output-trio.png`](../assets/output-trio.png) (PDF/A + HTML +
capsule, teal mark under the H2, JSON `b468b37f…`; trio/capsule panel still `b867db0a…` — shot lag, not a fake hash). Below-fold
shots: [`output-pdf.png`](../assets/output-pdf.png),
[`output-html.png`](../assets/output-html.png). The demo-image gap is **closed**.
Capsule hashes travel with the files.

![PDF/A, HTML, and capsule from examples/letter.md — teal harbor mark under the H2](../assets/output-trio.png)

![PDF/A page from examples/letter.md — teal harbor mark under the H2](../assets/output-pdf.png)

![HTML from examples/letter.md — teal harbor mark under the H2 as img](../assets/output-html.png)

| Placement | What ships |
| --- | --- |
| PDF/A | PNG and JPEG layout and paint as PDF/A XObjects. |
| DOCX | PNG and JPEG as `a:blip`. |
| ODT | PNG and JPEG as `draw:image`. |
| Markdown | `![alt](src "title")`, including a relative path such as `docs/assets/mark.png`. |
| HTML `read` | data-URI `<img>`, plus lists, `<th>`/`<thead>`, `<blockquote>`, `<pre>`, `<a href>`, `<mark>`. |
| Hangul | Images **—**. |

HTML `read` recovers `<img src="data:image/png|jpeg;base64,…">` into
`InlineObject::Image`. Write emits `<img>`. `http(s):` `src` is skipped. This is
not a full HTML5 engine: no CSS layout, no JavaScript, no remote fetch.

## Hangul codecs

Hangul files are regional codecs on the same IR, not a second product. The
unique **yes** on the runtime matrix stands. Images on Hangul are not claimed.

## Not this product

- OCR / VLM / scanned-page reconstruction (MinerU, Docling, Marker).
- OmniDocBench scores. We do not publish them because we do not compete there.
- Spreadsheets, slides, a GUI, a cursor, or an undo stack.
- GPU as a requirement. Layout is integer CPU.
- WeasyPrint print-CSS tightness. Table/paragraph tightness vs WeasyPrint remains
  an open gap. Do not read this page as closing it.

Proof files for `examples/letter.md`: [PDF/A](../assets/letter.pdf),
[HTML](../assets/letter.html), [DOCX](../assets/letter.docx),
[ODT](../assets/letter.odt), [Markdown](../assets/letter.md),
[capsule](../assets/letter-capsule.json). SHA-256 lockstep:
input `b0f4423728db4238779b3bf6cbd238d4d0d637f733c078322be6ff23398bbcdf`,
plan `4b7360eb1b7a9bec20cc35d5c79f2156c22d2712ab62ededfb60e328b8f16d3a`,
output `b468b37f1c3d8da7e4aac7e1a8417b46c29a57863817f9df16e8d4a6cc89f0f8`.
`prove` plan/output hashes stay **distinct** (`928b4745…` / `2607bb6f…`) — not copied onto this capsule.
