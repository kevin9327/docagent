<p align="center">
  <img src="docs/assets/hero.svg" alt="DocAgent — picture in, PDF/A out. Capsule hashed. GPU-free. Not OCR." width="100%">
</p>

<p align="center">
  <a href="https://github.com/kevin9327/docagent/actions/workflows/ci.yml"><img src="https://github.com/kevin9327/docagent/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/rust-1.93-orange" alt="Rust 1.93">
  <img src="https://img.shields.io/badge/license-MIT-5eead4" alt="MIT">
  <img src="https://img.shields.io/badge/layout-integer%201%2F7200%22-0f766e" alt="Integer layout">
  <img src="https://img.shields.io/badge/capsule-SHA--256%20×3-134e4a" alt="Capsule hashes">
  <img src="https://img.shields.io/badge/GPU-not%20required-111827" alt="No GPU">
  <img src="https://img.shields.io/badge/OCR-not%20this%20product-1f2937" alt="Not OCR">
</p>

<p align="center"><b>Picture in. PDF/A out. Capsule hashed. GPU-free.</b><br>
A PNG or JPEG in Markdown, HTML, DOCX, or ODT becomes a PDF/A XObject.<br>
Same fonts → same bytes. Agents call <code>Command::Convert</code>. We embed the file you already have — we do not OCR it.</p>

<p align="center">
  <a href="#install--run">Install</a> ·
  <a href="#vs-the-locked-ten">vs the field</a> ·
  <a href="#what-ships">What ships</a> ·
  <a href="#command--query--event">API</a> ·
  <a href="docs/src/intro.md">Docs</a>
</p>

<p align="center">
  <img src="docs/assets/mark.png" alt="Harbor mark — the PNG that goes in" width="96">
</p>

```markdown
![Harbor mark](docs/assets/mark.png)
```

```bash
docagent convert examples/letter.md --pdf out.pdf --html out.html --capsule cap.json
```

<p align="center"><sub>That line is in <code>examples/letter.md</code>. Convert paints the teal harbor mark under the H2 as a PDF/A XObject. Not a scan. Not a VLM guess. Not OmniDocBench.</sub></p>

<p align="center">
  <img src="docs/assets/output-trio.png" alt="Real convert of examples/letter.md: PDF/A page with the teal harbor mark under the H2, semantic HTML with the same mark, capsule SHA-256 ×3" width="100%">
</p>
<p align="center"><sub>Actual engine output of <code>examples/letter.md</code> — PDF/A, HTML, capsule. The teal harbor mark sits under the H2 on the page.<br>
<strong>Images row:</strong> PNG + JPEG are <strong>yes</strong> on Markdown / HTML / PDF/A / DOCX / ODT (PDF/A XObject, DOCX <code>a:blip</code>, ODT <code>draw:image</code>, HTML <code>&lt;img&gt;</code>). Hangul images are <strong>—</strong>.</sub></p>

<p align="center">
  <img src="docs/assets/output-pdf.png" alt="PDF/A page from examples/letter.md — teal harbor mark under the H2 as a PDF/A XObject" width="100%">
</p>
<p align="center"><sub>PDF/A from the integer layout engine. The teal mark is under <em>Delivery confirmation — order 48291</em>. GPU-free. Same fonts → same bytes.</sub></p>

---

<p align="center">
  <img src="docs/assets/pipeline.svg" alt="Picture in: Markdown DOCX ODT HTML with PNG or JPEG. One integer engine. PDF/A out with that picture as an XObject. Three capsule hashes. Not OCR." width="100%">
</p>

<p align="center">
  <img src="docs/assets/ships.svg" alt="PNG and JPEG on Markdown, HTML, PDF/A, DOCX, and ODT become PDF/A XObjects. Hangul images are a dash. Not OCR, not VLM, not OmniDocBench." width="100%">
</p>

## Why this exists

MinerU, Docling, Marker, and Unstructured turn documents into tokens for models.
Pandoc, python-docx, WeasyPrint, ONLYOFFICE, and LibreOffice turn documents into other documents.
**DocAgent is the runtime in the middle that an agent can call, replay, and prove.**

Picture in, PDF/A out is the first-screen job. MinerU reads a scan. DocAgent embeds a PNG.

| An agent needs | DocAgent |
| --- | --- |
| One typed call, not a GUI | `Command::Convert` |
| Proof it ran the same file | capsule: input / plan / output SHA-256 |
| Same fonts → same PDF twice | integer layout at 1/7200 inch |
| Word, LibreOffice, Markdown, print | DOCX · ODT · MD · HTML · PDF/A |
| A picture that survives convert | PNG + JPEG → PDF/A XObject on those five codecs. Hangul: **—** |
| No GPU bill, no VLM drift | CPU, deterministic. Not OCR. |

People still receive a normal file. The agent receives a capsule.

## vs the locked ten

Locked against the ten most-starred document repositories an agent actually meets
(star counts 2026-09-06). This is the **agent-runtime** matrix, not an OCR bake-off.
DocAgent does not claim MinerU's VLM scores. MinerU does not ship a replayable capsule.
No OmniDocBench numbers live on this page.

| Tool | Job | C/Q/E | Capsule | Same bytes | i32 layout | GPU-free | Hangul |
| --- | --- | :---: | :---: | :---: | :---: | :---: | :---: |
| **DocAgent** | Agent document runtime | **yes** | **SHA-256 ×3** | **yes** | **1/7200"** | **yes** | **yes** |
| [MinerU](https://github.com/opendatalab/MinerU) 79k | PDF/Office → LLM markdown | | | | | | |
| [Docling](https://github.com/docling-project/docling) 66k | Gen-AI parse + export | | | | | optional | |
| [Pandoc](https://github.com/jgm/pandoc) 46k | Universal markup convert | | | | | **yes** | |
| [Marker](https://github.com/datalab-to/marker) 40k | PDF → markdown/JSON | | | | | | |
| [Unstructured](https://github.com/Unstructured-IO/unstructured) 15k | Document ETL for LLMs | | | | | | |
| [PyMuPDF](https://github.com/pymupdf/PyMuPDF) 11k | PDF extract/convert | | | | | **yes** | |
| [WeasyPrint](https://github.com/Kozea/WeasyPrint) 10k | HTML/CSS → PDF | | | | | **yes** | |
| [ONLYOFFICE](https://github.com/ONLYOFFICE/DocumentServer) 6.9k | Collaborative office suite | | | | | **yes** | |
| [python-docx](https://github.com/python-openxml/python-docx) 5.7k | Word DOCX read/write | | | | | **yes** | |
| [LibreOffice](https://github.com/LibreOffice/core) 4.3k | Headless office convert | | | | | **yes** | |

**North star:** drop `examples/letter.md` (or DOCX/ODT) that contains a PNG or JPEG, and get PDF/A + HTML + Markdown + DOCX + ODT whose bytes match the next run, with three hashes in the capsule. The picture is a PDF/A XObject. Not OCR.

Images: **yes** PNG + JPEG on Markdown / HTML / PDF/A / DOCX / ODT. **—** on Hangul.
The demo letter is `![Harbor mark](docs/assets/mark.png)`. Convert shots on this page show the teal mark under the H2.

## What ships

Honest codec coverage — not a wishlist. Full table: [`docs/src/capabilities.md`](docs/src/capabilities.md).

| Mark | Markdown | HTML | PDF/A | DOCX | ODT | Hangul |
| --- | :---: | :---: | :---: | :---: | :---: | :---: |
| Lists (ul/ol, nested) | **yes** | `read` ul/ol/tasks | markers | `w:numPr` | `text:list` | **yes** |
| Links | `[text](url)` | `<a href>` | URI `/Link` | `w:hyperlink` | `text:a` | **yes** |
| Strike | `~~text~~` | `<s>` | fill | `w:strike` | line-through | **yes** |
| Quotes | `>` | `<blockquote>` | quote bar | Quote | Quote | **yes** |
| Code | `` `span` `` / fence | `<code>` / `<pre>` | fills | Code | Tcode | **yes** |
| Thematic break | `---` | `<hr/>` | rule fill | HorizontalLine | HorizontalLine | **yes** |
| Tasks | `- [x]` | checkbox | task box | `[x]` | `[x]` | **yes** |
| Highlight | `==mark==` | `<mark>` | fill | `w:highlight` | Tmark | **yes** |
| Super / sub | `^ ^` / `~ ~` | `<sup>` / `<sub>` | baseline | `w:vertAlign` | Tsuper / Tsub | **yes** |
| Underline | `__text__` | `<u>` | fill | `w:u` | Tunder | **yes** |
| **Images (PNG, JPEG)** | `![alt](…)` | `<img>` | **XObject** | `a:blip` | `draw:image` | **—** |
| Table header | GFM `\| --- \|` | `<th>` / `<thead>` | header fill | `w:tblHeader` | `THcell` | **yes** |

**Images are the first-class row.** PNG and JPEG round-trip on Markdown, HTML, PDF/A, DOCX, and ODT. They stay **—** on Hangul. That dash is real. The demo letter is `![Harbor mark](docs/assets/mark.png)` — picture in. PDF/A out paints it as an XObject under the H2.

HTML `read` recovers data-URI `<img>`, lists, `<th>`/`<thead>`, `<blockquote>`, `<pre>`, `<a href>`, and `<mark>` into the same IR. `http(s):` `src` is skipped. This is not a full HTML5 engine.

No spreadsheet. No slides. No cursor. No undo stack. No OCR. Flow documents only.

## Install / run

```bash
git clone https://github.com/kevin9327/docagent
cd docagent
cargo test --workspace
cargo run -p docagent-cli -- convert examples/letter.md \
  --pdf out.pdf --html out.html --odt out.odt --docx out.docx --capsule cap.json
cargo run -p docagent-cli -- prove examples/letter.md
```

`prove` converts twice (PDF/A + HTML) and exits 0 only when the bytes and the three SHA-256 hashes match. Fixture: [`docs/assets/letter-prove.json`](docs/assets/letter-prove.json).

Binaries: `docagent` (CLI) · `docagentd` (daemon) · MCP + WIT adapters.

```rust
use docagent_api::{Command, Engine, ExportTarget};

let mut engine = Engine::new();
let out = engine.execute(Command::Convert {
    input: std::fs::read("examples/letter.md")?,
    input_kind: None,
    targets: vec![ExportTarget::PdfA, ExportTarget::Html, ExportTarget::Markdown],
})?;
// out.capsule.{input_hash, plan_hash, output_hash}
```

## Same bytes twice

Not a slogan. Two `Command::Convert` calls on [`examples/letter.md`](examples/letter.md) (PDF/A + HTML) produced the same files. The test is `letter_md_is_byte_identical_across_two_runs`.

<p align="center">
  <img src="docs/assets/output-rerun.png" alt="Two converts of examples/letter.md with identical input, plan, and output SHA-256 hashes" width="100%">
</p>
<p align="center"><sub>Run 1 and run 2. <code>identical: true</code>. PDF bytes equal. HTML bytes equal. GPU-free. Deterministic.</sub></p>

| Hash | SHA-256 |
| --- | --- |
| input | `b0f4423728db4238779b3bf6cbd238d4d0d637f733c078322be6ff23398bbcdf` |
| plan | `4b7360eb1b7a9bec20cc35d5c79f2156c22d2712ab62ededfb60e328b8f16d3a` |
| output | `9f5df3a92ef707fade241aac183d90e21c50922bacf76d2ad13eb282a63bebfd` |

These are the hashes in [`letter-capsule.json`](docs/assets/letter-capsule.json). MinerU and Docling do not ship this.

## Convert shots

Files from [`examples/letter.md`](examples/letter.md): [PDF/A](docs/assets/letter.pdf) · [HTML](docs/assets/letter.html) · [DOCX](docs/assets/letter.docx) · [ODT](docs/assets/letter.odt) · [Markdown](docs/assets/letter.md) · [capsule JSON](docs/assets/letter-capsule.json).

<p align="center">
  <img src="docs/assets/output-html.png" alt="Semantic HTML produced by DocAgent from the same letter, teal harbor mark under the H2" width="100%">
</p>
<p align="center"><sub>HTML from the same IR. The teal mark is under the H2 as <code>&lt;img&gt;</code>. <code>read</code> recovers data-URI images. Not HTML5. GPU-free.</sub></p>

<p align="center">
  <img src="docs/assets/output-capsule.png" alt="Capsule JSON with input, plan, and output SHA-256 hashes" width="100%">
</p>
<p align="center"><sub>Capsule: input / plan / output SHA-256. Replay is evidence.</sub></p>

<p align="center">
  <img src="docs/assets/convert-story.jpg" alt="Picture in, PDF/A out: a photograph becomes a PDF/A page; the metal token is the capsule" width="100%">
</p>
<p align="center"><sub>Picture in → PDF/A out. The metal token is the capsule: three SHA-256 hashes an agent can replay. Not a scanner.</sub></p>

<p align="center">
  <img src="docs/assets/capsule-page.jpg" alt="Laid-out page with a picture, bound to a three-hash capsule chain" width="100%">
</p>
<p align="center"><sub>One layout pass. The picture is in the page. Input, plan, and output each get a hash. Same fonts, same bytes.</sub></p>

| Surface | What it is |
| --- | --- |
| **IR** | Format-blind document model. Layout never sees “docx” or “hwp”. |
| **Layout** | One engine. Coordinates are `i32` at 1/7200 inch. `f32` is forbidden. |
| **PDF/A** | Archival print from the integer plan. PNG and JPEG are XObjects. Two runs, same fonts, same bytes. |
| **DOCX / ODT** | Word and LibreOffice files. Headings, lists, links, tables, PNG + JPEG (`a:blip` / `draw:image`). |
| **Markdown / HTML** | Agent-native text and semantic HTML. `![alt](src)`. HTML `<img>` write / data-URI `read`. Not HTML5. |
| **Capsule** | Three SHA-256 hashes on every Command so a retry is evidence, not hope. |
| **Hangul** | Regional codecs on the same IR, not the product name. Images on Hangul are still **—**. |

## Command / Query / Event

```text
Command  →  Convert | Import | Export | LayoutDocument
Query    →  EngineVersion | LastCapsule | LastDocument | CapsuleByInputHash
Event    →  CommandCompleted { capsule_id } | Warning
```

Every Command writes a capsule. CLI, daemon, MCP, and WIT are thin adapters over
the same `Engine`. See [`AGENTS.md`](AGENTS.md) and [`docs/src/intro.md`](docs/src/intro.md).

```mermaid
flowchart LR
  A[DOCX / ODT / MD / HTML + PNG/JPEG] --> B[docagent-model IR]
  B --> C[docagent-layout<br/>i32 @ 1/7200 in]
  C --> D[PDF/A XObject]
  C --> E[HTML]
  C --> F[DOCX / ODT / MD]
  B --> G[capsule.json<br/>input · plan · output]
```

## Crate graph

Codecs (`docx`, `odt`, `md`, `html`, `hwp5`, `hwpx`, `hwp3`, `hml`) do not import each other
and do not import layout. Layout, paint, raster, and PDF do not know the source format.

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p docagent-lint
cargo run -p docagent-conformance -- --require-win
```

## Docs

- [Intro](docs/src/intro.md)
- [What ships](docs/src/capabilities.md)
- [Format hops](docs/src/format-hops.md)
- [ADRs](docs/src/adr/README.md) — crate boundaries, integer layout, single engine, capsules

## License

MIT. See [LICENSE](LICENSE).
