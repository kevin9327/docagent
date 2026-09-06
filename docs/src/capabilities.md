# What ships

DocAgent is an **agent document runtime**. The GitHub page is scored against the
ten most-starred document repositories an agent actually meets (2026-09-06):
MinerU, Docling, Pandoc, Marker, Unstructured, PyMuPDF, WeasyPrint, ONLYOFFICE,
python-docx, LibreOffice.

This page is the honest matrix. It is not an OCR bake-off. There are no
OmniDocBench scores, no VLM accuracy claims, and no invented GPU numbers.

First-screen job: **picture in, PDF/A out.** A PNG or JPEG in Markdown, HTML,
DOCX, or ODT becomes a PDF/A XObject. We embed the file. We do not OCR it.
The demo convert paints the teal harbor mark under the H2. The demo-image
gap is **closed**. Capsule hashed. GPU-free.

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
bytes, identical input / plan / output SHA-256.

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
| Table header row | GFM `\| --- \|` | `<th>` / `<thead>` | header fill | `w:tblHeader` | `THcell` | **yes** |
| **Images (PNG, JPEG)** | `![alt](…)` | `<img>` | **XObject** | `a:blip` | `draw:image` | **—** |

Images are **yes** (PNG and JPEG) on Markdown, HTML, PDF/A, DOCX, and ODT. They
stay **—** on Hangul.

`examples/letter.md` includes `![Harbor mark](docs/assets/mark.png)`. First-screen
convert shots — [`output-trio.png`](../assets/output-trio.png),
[`output-pdf.png`](../assets/output-pdf.png),
[`output-html.png`](../assets/output-html.png) — show the teal mark under the H2
on the PDF/A page and the HTML page. The demo-image gap is **closed**. Capsule
hashes travel with the files.

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

Proof files for `examples/letter.md`: [PDF/A](../assets/letter.pdf),
[HTML](../assets/letter.html), [DOCX](../assets/letter.docx),
[ODT](../assets/letter.odt), [Markdown](../assets/letter.md),
[capsule](../assets/letter-capsule.json). SHA-256 lockstep:
input `b0f4423728db4238779b3bf6cbd238d4d0d637f733c078322be6ff23398bbcdf`,
plan `4b7360eb1b7a9bec20cc35d5c79f2156c22d2712ab62ededfb60e328b8f16d3a`,
output `b4b3843b18793d3fa50e43706583fc7bd388f83fef04ecabaccb2cb6cf084fc1`.
