# What ships

DocAgent is an **agent document runtime**. The GitHub page is scored against the
ten most-starred document repositories an agent actually meets (2026-09-06):
MinerU, Docling, Pandoc, Marker, Unstructured, PyMuPDF, WeasyPrint, ONLYOFFICE,
python-docx, LibreOffice.

This page is the honest matrix. It is not an OCR bake-off. There are no
OmniDocBench scores, no VLM accuracy claims, and no invented GPU numbers.

## Runtime axes

These are the axes DocAgent wins on. Blank cells mean the other project does
not ship that axis as a product feature.

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
teaser. Hangul quotes, fenced code, tasks, highlight, and first-class lists
are not claimed.

| Mark | Markdown | HTML | PDF/A | DOCX | ODT | Hangul |
| --- | :---: | :---: | :---: | :---: | :---: | :---: |
| Lists (ul/ol, nested) | **yes** | **yes** | painted markers | `w:numPr` | `text:list` | — |
| Links | `[text](url)` | `<a href>` | URI `/Link` | `w:hyperlink` | `text:a` | **yes** |
| Strike | `~~text~~` | `<s>` | fill | `w:strike` | line-through | CHAR_SHAPE (not HWP 3) |
| Quotes | `>` | `<blockquote>` | quote bar | Quote | Quote | — |
| Code spans / fences | `` ` `` / ` ``` ` | `<code>` / `<pre>` | fills | Code / CodeBlock | Tcode / CodeBlock | — |
| Tasks | `- [x]` | checkbox | task box | `[x]` | `[x]` | — |
| Highlight | `==mark==` | `<mark>` | fill | `w:highlight` | Tmark | — |
| Superscript / subscript | `^ ^` / `~ ~` | `<sup>` / `<sub>` | baseline shift | `w:vertAlign` | Tsuper / Tsub | CHAR_SHAPE |
| Underline | `__text__` | `<u>` | fill | `w:u` | Tunder | CHAR_SHAPE |
| Table header row | GFM `\| --- \|` | `<th>` | header fill | `w:tblHeader` | `THcell` | — |
| Thematic break | `---` | `<hr/>` | rule fill | HorizontalLine | HorizontalLine | — |

## Hangul codecs

Hangul files are regional codecs on the same IR, not a second product.

| Codec | CHAR_SHAPE / charPr | Hyperlink |
| --- | --- | --- |
| HWP 5 | `HWPTAG_CHAR_SHAPE` attr: bold, italic, underline, strike, super, sub | `%hlk` field begin/end |
| HWPX | `hh:charPr`: bold, italic, underline, strikeout, supscript, subscript | `hp:fieldBegin type="HYPERLINK"` |
| HML | `CHARSHAPE`: BOLD, ITALIC, UNDERLINE, STRIKEOUT, SUPERSCRIPT, SUBSCRIPT | `FIELDBEGIN Type="Hyperlink"` |
| HWP 3 | char-shape attr bits: bold, italic, underline, super, sub (no strike bit) | control char 10 (`other_options & 0x10`) + additional-info TagID 3 (617-byte kchar URL) |

Not yet on Hangul: quotes, `code_block`, task lists, highlight, thematic breaks,
first-class numbering.

## Not this product

- OCR / VLM / scanned-page reconstruction (MinerU, Docling, Marker).
- OmniDocBench scores. We do not publish them because we do not compete there.
- Spreadsheets, slides, a GUI, a cursor, or an undo stack.
- GPU as a requirement. Layout is integer CPU.

Proof files for `examples/letter.md`: [PDF/A](../assets/letter.pdf),
[HTML](../assets/letter.html), [DOCX](../assets/letter.docx),
[ODT](../assets/letter.odt), [Markdown](../assets/letter.md),
[capsule](../assets/letter-capsule.json).
