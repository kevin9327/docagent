# DocAgent

DocAgent is a **document-only** runtime for agents. It is not an office suite and
not a Hangul-only tool. Agents open, draft, convert, and verify flow documents.
People receive PDF/A, DOCX, ODT, Markdown, or HTML.

**Picture in. PDF/A out. Capsule hashed. GPU-free.** A PNG or JPEG in the source
becomes a PDF/A XObject. Same fonts → same bytes. We embed the bytes you already
have. No OCR. No VLM.

MinerU, Docling, Marker, and Unstructured turn documents into tokens for models.
Pandoc, python-docx, WeasyPrint, ONLYOFFICE, and LibreOffice turn documents into
other documents. **DocAgent is the runtime in the middle that an agent can call,
replay, and prove.**

GitHub first screen (MinerU/Docling density for an **agent runtime**): 200px hero,
badges, one-line runtime strip, a collapsed unique-yes `<details>` (C/Q/E ·
capsule · `prove` — none of the locked ten), the harbor-mark convert trio
(`output-trio.png`, teal under the H2), and capsule SHA-256 ×3 lockstep with
[`letter-capsule.json`](../assets/letter-capsule.json)
**above** the vs-the-ten matrix. **Quickstart sits below that matrix**.

Unique vs the locked ten — MinerU, Docling, Pandoc, Marker, Unstructured,
PyMuPDF, WeasyPrint, ONLYOFFICE, python-docx, and LibreOffice do not ship an
agent API, a three-hash capsule, and byte-identical replay together:

- **Picture in, PDF/A out.** PNG/JPEG on Markdown, HTML, DOCX, ODT paint as a PDF/A XObject. We embed the bytes. We do not OCR.
- **Integer layout.** Coordinates are `i32` at 1/7200 inch. Same fonts → same bytes. GPU-free.
- **Hangul** is a regional codec on the same IR (images **—**), not the product name.
- **Not claimed:** OCR, VLM, OmniDocBench, WeasyPrint print-CSS tightness (table/paragraph still **open**).

![PDF/A, HTML, and capsule from examples/letter.md — teal harbor mark under the H2](../assets/output-trio.png)

`examples/letter.md` → PDF/A + HTML + capsule. Teal harbor mark under the H2. JSON `b468b37f…`; trio/capsule panel still `b867db0a…` (shots lag — not a fake hash). WeasyPrint tightness **not** claimed closed.

SHA-256 lockstep with [`letter-capsule.json`](../assets/letter-capsule.json):

| Hash | SHA-256 |
| --- | --- |
| input | `b0f4423728db4238779b3bf6cbd238d4d0d637f733c078322be6ff23398bbcdf` |
| plan | `4b7360eb1b7a9bec20cc35d5c79f2156c22d2712ab62ededfb60e328b8f16d3a` |
| output | `b468b37f1c3d8da7e4aac7e1a8417b46c29a57863817f9df16e8d4a6cc89f0f8` |

Trio/capsule shots still show `b867db0a…` (pixels `b867db0aa9125feb…`) while JSON is `b468b37f…` — shot lag, not a fake hash. `prove` writes a separate
[`letter-prove.json`](../assets/letter-prove.json); those plan/output hashes stay
**distinct** (`928b4745…` / `2607bb6f…`) from this convert capsule — do not copy
them onto the table above. Print-CSS tightness vs WeasyPrint is **not** claimed closed.

```bash
git clone https://github.com/kevin9327/docagent
cd docagent
cargo test --workspace
cargo run -p docagent-cli -- convert examples/letter.md \
  --pdf out.pdf --html out.html --odt out.odt --docx out.docx --capsule cap.json
cargo run -p docagent-cli -- prove examples/letter.md
```

`examples/letter.md` contains `![Harbor mark](docs/assets/mark.png)`. Convert paints
the teal harbor mark under the H2 as a PDF/A XObject. `prove` converts twice
(PDF/A + HTML) and exits 0 only when the bytes and the three SHA-256 hashes match.
The demo-image gap is **closed**.

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

Binaries: `docagent` (CLI) · `docagentd` (daemon) · MCP + WIT adapters over the same `Engine`.

```text
Command  →  Convert | Import | Export | LayoutDocument
Query    →  EngineVersion | LastCapsule | LastDocument | CapsuleByInputHash
Event    →  CommandCompleted { capsule_id } | Warning
```

Every Command writes a capsule. CLI, daemon, MCP, and WIT are thin adapters over
the same `Engine`.

| Family | Codec | Role |
| --- | --- | --- |
| Word / OOXML | DOCX | Default interchange with Microsoft Word. PNG + JPEG via `a:blip`. |
| OpenDocument | ODT | Default interchange with LibreOffice. PNG + JPEG via `draw:image`. |
| Web / notes | Markdown, HTML | Agent-native text (`![alt](src)`) and semantic HTML (`<img>`). HTML `read` recovers data-URI images. Not a full HTML5 engine. |
| Print | PDF/A | Archival, byte-identical on two runs. PNG and JPEG paint as XObjects. |
| Regional | HWP 5, HWPX, HWP 3, HML | East-Asian office formats, same IR. Images are **not** claimed on Hangul. |

One integer IR (lengths in 1/7200 inch). One layout engine. Command / Query / Event
is the only API.

| Surface | What it is |
| --- | --- |
| **IR** | Format-blind document model. Layout never sees “docx” or “hwp”. |
| **Layout** | One engine. Coordinates are `i32` at 1/7200 inch. `f32` is forbidden. |
| **PDF/A** | Archival print from the integer plan. PNG and JPEG are XObjects. Two runs, same fonts, same bytes. |
| **DOCX / ODT** | Word and LibreOffice files. Headings, lists, links, tables, PNG + JPEG (`a:blip` / `draw:image`). |
| **Markdown / HTML** | Agent-native text and semantic HTML. `![alt](src)`. HTML `<img>` write / data-URI `read`. Not HTML5. |
| **Capsule** | Three SHA-256 hashes on every Command so a retry is evidence, not hope. |
| **Hangul** | Regional codecs on the same IR, not the product name. Images on Hangul are still **—**. |

Convert shots plus the mark fixture:
[`output-trio.png`](../assets/output-trio.png),
[`output-pdf.png`](../assets/output-pdf.png),
[`output-html.png`](../assets/output-html.png),
[`mark.png`](../assets/mark.png).

![PDF/A page from examples/letter.md — teal harbor mark under the H2](../assets/output-pdf.png)

![HTML from examples/letter.md — teal harbor mark under the H2 as img](../assets/output-html.png)

No GUI. No spreadsheet or presentation models. No OCR scores. No OmniDocBench
numbers.

See [What ships](capabilities.md) for the codec matrix versus MinerU, Docling,
Pandoc, Marker, Unstructured, PyMuPDF, WeasyPrint, ONLYOFFICE, python-docx, and
LibreOffice.
