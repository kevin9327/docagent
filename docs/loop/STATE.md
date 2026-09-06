# DocAgent #1 loop state

Last cycle: 9 (2026-09-06) — PDF synthetic bold

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Layout splits wrap lines into per-run `LineFrag`s carrying `CharStyle.bold` / `italic`.
- Paint `Op::Text` forwards both flags.
- PDF/A draws a second fill offset ~3% of em on bold spans (headings and **GPU-free**). Bundle is still Noto Sans Regular; italic is Regular-only this cycle.
- Prove panel and convert screenshots refreshed. `shot_outputs.py` now writes `output-rerun.png` from `letter-prove.json`.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `5f33b7f2389a3c4a4f53b638808a8e958305cc8f8a77b976d03f3d41f3d4a85f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `ea648d9068cf4b27356ac594e82211aa1d3d16c163c80b087e62aff42e94a0b6`

## Next

1. PDF italic: skew transform or italic face on `CharStyle.italic` (`_byte-for-byte_` in the letter).
2. Or PDF fills beyond table grids.
3. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
