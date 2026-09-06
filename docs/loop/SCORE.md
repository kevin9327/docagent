# Score vs the locked ten (2026-09-06, shallow)

| Axis | DocAgent | Best of the ten | Gap |
| --- | --- | --- | --- |
| GitHub first screen | Picture-in copy; letter.md has Harbor mark; `output-*.png` still text-only | MinerU/Docling photos | GitHub-image gap **open** — shots show no picture |
| Agent API + capsule | Command + SHA-256×3 | none | none — keep |
| Same bytes (`prove`) | yes | none | none — keep |
| Demo letter looks real | source has `![Harbor mark]`; GitHub frames do not | Pandoc/WeasyPrint | demo must rewrite `output-*.png` |
| Images in the demo | letter.md **has** `![`; convert shots **do not** | yes | captions stay honest: no picture in those PNGs |
| DOCX/ODT/MD/HTML/PDF/A | five codecs; PNG/JPEG engine yes | Pandoc | engine yes; Hangul images **—**; **stop deepening Hangul** |

**This hour’s six (no trivia, no new IR):**

1. landing — first screen already sells picture in, PDF/A out. Captions must say the convert shots are still text-only. Do not invent OCR. Do not claim the GitHub-image gap is closed.
2. pdf — Harbor mark must paint as a PDF/A XObject on the letter page. No wrap-mode archaeology.
3. docx — `letter.docx` from that letter includes the picture (`a:blip`).
4. odt — same for `letter.odt` (`draw:image`).
5. mdhtml — `![alt](docs/assets/mark.png)` round-trips; HTML write is `<img>`, not a broken link.
6. demo — convert + prove `examples/letter.md`; refresh `docs/assets/letter.*` and `output-*.png` so GitHub shows the mark.

GitHub-image gap stays open until those shots show the mark. Then the next gap is PDF page quality vs WeasyPrint — still no new IR.
