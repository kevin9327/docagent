# DocAgent

DocAgent is a **document-only** runtime for agents. It is not an office suite and
not a Hangul-only tool. Agents open, draft, convert, and verify flow documents.
People receive PDF/A, DOCX, ODT, Markdown, or HTML. Hangul files use the same
pipeline.

The contract a stranger should see on the first screen: **deterministic convert,
three-hash capsule, GPU-free.** Same fonts → same bytes. No VLM.

Formats, in the order agents usually meet them:

| Family | Codec | Role |
| --- | --- | --- |
| Word / OOXML | DOCX | Default interchange with Microsoft Word (`Heading1`/`Heading2`, `Quote`, `Code`, `CodeBlock`, lists, `w:hyperlink`, `w:strike`, `w:highlight`, `w:vertAlign`, `a:blip` PNG + JPEG, `wp:wrapSquare`, `wp:wrapNone` + `behindDoc`, `wp:wrapNone` in front) |
| OpenDocument | ODT | Default interchange with LibreOffice (`text:h`, Quote, `Tcode`, `Tmark`, `Tunder`, `Tsuper`, `Tsub`, `Tlink`, `CodeBlock`, nested `text:list`, `draw:image` PNG + JPEG including table cells, `WrapMode::Tight` / `Gtight`, `svg:desc` alt) |
| Web / notes | Markdown, HTML | Agent-native text (`> quote`, `` `code` ``, fences, `==mark==`, `__underline__`, `^super^`, `~sub~`, `- [x]`, `~~strike~~`, `[text](url)`, `![alt](src "title")`) and semantic HTML (`<blockquote>`, `<pre>`, `<mark>`, `<u>`, `<sup>`, `<sub>`, `<s>`, checkbox, `<a href>`, `<img src="data:…">`, `<ul>`/`<ol>`/`<ul class="tasks">`, `<th>`/`<thead>`). HTML `read` recovers data-URI images, lists, `<th>`/`<thead>`, `<blockquote>`, `<pre>`, `<a href>`, and `<mark>` (Pandoc-class import). Not a full HTML5 engine. |
| Print | PDF/A | Archival, byte-identical on two runs. Marks are painted fills; PNG and JPEG are XObjects, including `Block::Float(Float::Image)` and table-cell images (oversized floats clamp to content width; cell images clamp to cell width). `Section.header` / `footer` text paints in the margin bands. |
| Regional | HWP 5, HWPX, HWP 3, HML | East-Asian office formats, same IR. CHAR_SHAPE / `hh:charPr` / `CHARSHAPE` / HWP 3 attr bits map bold, italic, underline, super, sub (strike on HWP 5 / HWPX / HML). Highlight is CHAR_SHAPE shade. Table header rows on HWP 5 / HWPX / HML (not HWP 3). Hyperlinks: HWP 5 `%hlk`, HWPX `hp:fieldBegin type="HYPERLINK"`, HML `FIELDBEGIN Type="Hyperlink"`, HWP 3 control-10 + TagID 3. Para styles: `Quote`, `CodeBlock`, `HorizontalLine`, `Task`, `Bullet`, `Number`. Images are **not** claimed on Hangul. |

One integer IR (lengths in 1/7200 inch). One layout engine. Command / Query / Event
is the only API. CLI `docagent`, daemon `docagentd`, MCP, and WIT are thin adapters.
Every Command writes a three-hash capsule (input, plan, output).

```
docagent convert examples/letter.md --pdf out.pdf --html out.html --md out.md --odt out.odt --capsule cap.json
docagent prove examples/letter.md
```

No GUI. No spreadsheet or presentation models. No OCR scores.

See [What ships](capabilities.md) for the codec matrix versus MinerU, Docling,
Pandoc, Marker, Unstructured, PyMuPDF, WeasyPrint, ONLYOFFICE, python-docx, and
LibreOffice.
