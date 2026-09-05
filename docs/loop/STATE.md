# DocAgent #1 loop state

Last cycle: 2 (2026-09-06) — real convert screenshots + wrap/ligature/table layout

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- `examples/letter.md` is the copy-paste sample (README command no longer points at a missing `letter.docx`).
- Converted it with `cargo run -p docagent-cli -- convert examples/letter.md`.
- Committed actual artifacts: `docs/assets/letter.pdf`, `letter.html`, `letter-capsule.json`.
- Committed screenshots of those artifacts: `output-pdf.png`, `output-html.png`, `output-capsule.png`, `output-trio.png`.
- README above-the-fold now has the trio; **What you get** leads with real PDF/HTML/capsule, then the photographs.
- `docagent-cli` `default-run = "docagent"` so the README `cargo run -p docagent-cli -- convert ...` line works.

Engine (needed for an honest PDF screenshot, not a broken wrap):
- Shaping maps rustybuzz clusters onto characters so ligatures no longer drop the last letters (`48291`, `hoping`).
- Line breaking prefers spaces (no `fo/nts`).
- Markdown tables stretch to the content width; first row is a header.
- Markdown headings carry size (18pt / 14pt). HTML emits `<h1>`/`<h2>`.

## Next

1. Table borders in PDF/A (HTML has them; the PDF page still looks like tab-separated columns).
2. Paint `FillRect` borders from layout so the PDF screenshot matches the HTML table.
3. Then the next matrix gap: byte-identical rerun proof on the README (two-run hash equality fixture), still no invented OmniDocBench numbers.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
