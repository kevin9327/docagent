# Score vs the locked ten (2026-09-06)

| Axis | DocAgent | Best of the ten | Gap |
| --- | --- | --- | --- |
| GitHub first screen | 200px hero + unique-yes `<details>` + trio + hashes **above** vs-ten; Quickstart **below** | MinerU/Docling visual | keep lockstep; hashes = capsule `b468b37f…`; trio/capsule shots still `b867db0a…` (pixels `b867db0aa9125feb…`, lag — not a fake hash) |
| Agent API + capsule | yes | none | keep |
| Same bytes (`prove`) | yes | none | keep; prove stays **distinct** (`928b4745…` / `2607bb6f…`) — named on README/docs; do not copy onto capsule |
| Letter page | 24pt mark under H2, table painted | WeasyPrint | **open** — table/paragraph vs WeasyPrint |
| Images in demo | yes | Pandoc/python-docx | keep in all five first-class codecs |
| Hangul | unique yes | none of the ten | **stop deepening** |

## Standing six (always fill; never idle)

1. **landing** — MinerU-density first screen. Hashes = `letter-capsule.json`. Do not claim WeasyPrint closed.
2. **pdf** — WeasyPrint letter page: table cell padding, gap after the mark, type. No wrap. No Hangul.
3. **docx** — `letter.md` → DOCX looks like Word: heading, table, visible `a:blip` image. python-docx/Pandoc axis.
4. **odt** — `letter.md` → ODT looks like LibreOffice: heading, table, visible `draw:image`.
5. **mdhtml** — HTML of the letter: `<img>` display size matches the PDF 24pt floor; readable article CSS (WeasyPrint input).
6. **demo** — convert + prove + `output-*.png` after any engine change. Do not fake hashes.
