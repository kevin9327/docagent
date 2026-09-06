# DocAgent

DocAgent is a **document-only** runtime for agents. It is not an office suite and
not a Hangul-only tool. Agents open, draft, convert, and verify flow documents.
People receive PDF/A, DOCX, ODT, Markdown, or HTML. Hangul files use the same
pipeline.

Formats, in the order agents usually meet them:

| Family | Codec | Role |
| --- | --- | --- |
| Word / OOXML | DOCX | Default interchange with Microsoft Word (`Heading1`/`Heading2`, `Quote`, `Code`, `CodeBlock`, `HorizontalLine`) |
| OpenDocument | ODT | Default interchange with LibreOffice (`text:h` 18pt/14pt, Quote, `Tcode`, `Tmark`, `Tunder`, `CodeBlock`, `HorizontalLine`) |
| Web / notes | Markdown, HTML | Agent-native text (`> quote`, `` `code` ``, ` ```fence``` `, `==mark==`, `__underline__`, `- [x]`, `---`) and semantic HTML (`<blockquote>`, `<pre>`, `<mark>`, `<u>`, checkbox, `<hr/>`) |
| Print | PDF/A | Archival, byte-identical on two runs |
| Regional | HWP 5, HWPX, HWP 3, HML | East-Asian office formats, same IR |

One integer IR (lengths in 1/7200 inch). One layout engine. Command / Query / Event
is the only API. CLI `docagent`, daemon `docagentd`, MCP, and WIT are thin adapters.
Every Command writes a three-hash capsule (input, plan, output).

```
docagent convert examples/letter.md --pdf out.pdf --html out.html --md out.md --odt out.odt --capsule cap.json
docagent prove examples/letter.md
```

No GUI. No spreadsheet or presentation models.
