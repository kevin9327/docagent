# DocAgent

DocAgent is a **document-only** runtime for agents. It is not an office suite and
not a Hangul-only tool. Agents open, draft, convert, and verify flow documents.
People receive PDF/A, DOCX, ODT, Markdown, or HTML.

The contract a stranger should see on the first screen: **picture in, PDF/A out.
Capsule hashed. GPU-free.** A PNG or JPEG in the source becomes a PDF/A XObject.
Same fonts → same bytes. We embed the bytes you already have. No OCR. No VLM.

Formats, in the order agents usually meet them:

| Family | Codec | Role |
| --- | --- | --- |
| Word / OOXML | DOCX | Default interchange with Microsoft Word. PNG + JPEG via `a:blip`. |
| OpenDocument | ODT | Default interchange with LibreOffice. PNG + JPEG via `draw:image`. |
| Web / notes | Markdown, HTML | Agent-native text (`![alt](src)`) and semantic HTML (`<img>`). HTML `read` recovers data-URI images. Not a full HTML5 engine. |
| Print | PDF/A | Archival, byte-identical on two runs. PNG and JPEG paint as XObjects. |
| Regional | HWP 5, HWPX, HWP 3, HML | East-Asian office formats, same IR. Images are **not** claimed on Hangul. |

One integer IR (lengths in 1/7200 inch). One layout engine. Command / Query / Event
is the only API. CLI `docagent`, daemon `docagentd`, MCP, and WIT are thin adapters.
Every Command writes a three-hash capsule (input, plan, output).

```
docagent convert examples/letter.md --pdf out.pdf --html out.html --md out.md --odt out.odt --capsule cap.json
docagent prove examples/letter.md
```

```markdown
![Harbor mark](docs/assets/mark.png)
```

That line is in `examples/letter.md`. Picture in, PDF/A out. Convert shots in
`docs/assets/output-*.png` are still text-only — no Harbor mark on those
frames. The source has the picture; the screenshots have not been rewritten.

No GUI. No spreadsheet or presentation models. No OCR scores.

See [What ships](capabilities.md) for the codec matrix versus MinerU, Docling,
Pandoc, Marker, Unstructured, PyMuPDF, WeasyPrint, ONLYOFFICE, python-docx, and
LibreOffice.
