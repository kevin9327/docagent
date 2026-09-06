# How DocAgent becomes #1

Do not implement random codec trivia. Every agent task must move a score
against the locked ten:

MinerU, Docling, Pandoc, Marker, Unstructured, PyMuPDF, WeasyPrint,
ONLYOFFICE, python-docx, LibreOffice.

## What “#1” means

Strangers on github.com/kevin9327/docagent pick this over those ten **as an
agent document runtime**. Not as OCR. Not as an office suite. Not as Hangul.

Win on: Command/Query/Event, capsule SHA-256×3, same bytes twice, integer
layout, GPU-free, first-class DOCX/ODT/MD/HTML/PDF/A, a README as dense as
MinerU/Docling with **real** convert shots.

Hangul is a regional codec. It is already a unique yes on the matrix. Do not
deepen Hangul (lists, shade, wrap, TagID) unless the hourly score says we
lose a first-class axis.

## Hourly score (shallow)

Once per hour, rewrite `docs/loop/SCORE.md` in under 30 lines:

| Axis | DocAgent | Best of the ten | Gap |
| --- | --- | --- | --- |
| GitHub first screen | | MinerU/Docling visual | |
| Agent API + capsule | yes | no | |
| Same bytes (`prove`) | yes | no | |
| Demo letter looks real | | Pandoc/WeasyPrint | |
| Images in the demo | missing if letter.md has no image | yes | |
| DOCX/ODT/MD/HTML/PDF/A | | Pandoc | |

No OmniDocBench. No invented OCR. Then pick **at most six** next tasks, one
per lane, from the queue below. Skip a lane if it has no queue item.

## Queue (do these, in order)

1. **landing** — `examples/letter.md` contains `![…](docs/assets/mark.png)` (or
   a data URI). Refresh `docs/assets/output-*.png`, `letter.pdf/html`, README
   trio. First screen must show a picture, like MinerU.
2. **pdf** — That image and the letter page paint cleanly in PDF/A (size,
   wrap, no overlap). Screenshots follow.
3. **docx** — `docs/assets/letter.docx` from that letter includes the picture.
4. **odt** — same for `letter.odt`.
5. **mdhtml** — markdown image path round-trips; HTML `<img>` in `letter.html`.
6. **demo** — `docagent convert` + `prove` on `examples/letter.md`; write
   capsule/prove JSON. Do not invent features.

Never leave a lane empty. If a queue item already shipped, use SCORE.md
standing six. Idle slots lose to MinerU. 1 out, 1 in — refill the same
turn a slot hits `done`. Do not wait 15 minutes.

## Forbidden unless SCORE names it

Wrap-mode variants, Hangul CHAR_SHAPE extras, TagIDs, HWP3 trivia, new IR
fields, OCR, GPU, spreadsheets.
