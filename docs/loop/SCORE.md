# Score vs the locked ten (2026-09-06, shallow)

| Axis | DocAgent | Best of the ten | Gap |
| --- | --- | --- | --- |
| GitHub first screen | Harbor-mark convert shots under the H2 | MinerU/Docling photos | **closed** |
| Agent API + capsule | Command + SHA-256×3 | none | none — keep |
| Same bytes (`prove`) | identical: true | none | none — keep |
| Demo letter looks real | tightness pass (`288c7ba` + shots) | WeasyPrint / MinerU | **open** — table/paragraph tightness vs WeasyPrint |
| Images in the demo | letter.md `![` and shots paint the mark | yes | **closed** |
| DOCX/ODT/MD/HTML/PDF/A | five codecs; PNG/JPEG yes | Pandoc | Hangul images **—**; **stop deepening Hangul** |

**This hour’s six (no trivia, no new IR, no invented numbers, no OCR):**

1. landing — first screen already shows the mark. Keep hashes lockstep with `letter-capsule.json`. Do not claim the WeasyPrint gap is closed. No OmniDocBench.
2. pdf — remaining letter-page tightness vs WeasyPrint: table cell padding and paragraph gap after the mark. Heading size/leading already locked — do not rewrite those tests. No wrap-mode. No Hangul.
3. docx — do not invent work. Picture already `a:blip`.
4. odt — do not invent work. Picture already `draw:image`.
5. mdhtml — do not invent work. `![alt]` / `<img>` already round-trip.
6. demo — re-run `prove` so `letter-prove.json` is from this engine (`plan_hash` still looks stale vs capsule). Keep `letter.*` / `output-*.png` lockstep. Do not fake hashes.

No OmniDocBench. No OCR/VLM. Next gap is WeasyPrint table/paragraph tightness, not another screenshot disclaimer.
