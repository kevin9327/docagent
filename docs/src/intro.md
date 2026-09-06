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
| Word / OOXML | DOCX | Default interchange with Microsoft Word (`Heading1`/`Heading2`, `Quote`, `Code`, `CodeBlock`, lists, `w:hyperlink`, `w:strike`, `w:highlight`, `w:vertAlign`, `a:blip` images) |
| OpenDocument | ODT | Default interchange with LibreOffice (`text:h`, Quote, `Tcode`, `Tmark`, `Tunder`, `Tsuper`, `Tsub`, `Tlink`, `CodeBlock`, `text:list`, `draw:image`) |
| Web / notes | Markdown, HTML | Agent-native text (`> quote`, `` `code` ``, fences, `==mark==`, `__underline__`, `^super^`, `~sub~`, `- [x]`, `~~strike~~`, `[text](url)`, `![alt](…)`) and semantic HTML (`<blockquote>`, `<pre>`, `<mark>`, `<u>`, `<sup>`, `<sub>`, `<s>`, checkbox, `<a href>`, `<img>`) |
| Print | PDF/A | Archival, byte-identical on two runs. Marks are painted fills; images are XObjects. |
| Regional | HWP 5, HWPX, HWP 3, HML | East-Asian office formats, same IR. CHAR_SHAPE / `hh:charPr` / `CHARSHAPE` / HWP 3 attr bits map bold, italic, underline, super, sub (strike on HWP 5 / HWPX / HML). Hyperlinks: HWP 5 `%hlk`, HWPX `hp:fieldBegin type="HYPERLINK"`, HML `FIELDBEGIN Type="Hyperlink"`, HWP 3 control-10 + TagID 3. Para styles: `Quote`, `CodeBlock`, `HorizontalLine`, `Task`. Images, highlight, first-class lists, and table headers are **not** claimed on Hangul. |

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
