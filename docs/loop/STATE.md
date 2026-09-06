# DocAgent #1 loop state

Last cycle: 28 (2026-09-06) — highlight marks as amber PDF fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- IR `CharStyle.highlight` paints an amber `FillRect` (`Color::MARK` = `#fde68a`) behind the run.
- Markdown reads/writes `==text==`. HTML emits `<mark>`. DOCX keeps `w:highlight w:val="yellow"`. ODT keeps `Tmark` `fo:background-color="#fde68a"`.
- Fixture: `==three hashes==` in the letter body so the GitHub PDF/HTML screenshots show the mark.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `c318f58544da892f4cb6d7e99025a08017f4efb0be140e29106592e909cf482a`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `e89fa5cb1f3d79a7a0d2843d34b508c51ef3d5184eaedae7c1e25221db3092a0`

Plan hash matches cycle 21–27 (plan is command + targets + fonts, not the paint tree). Input and output hashes changed with the fixture and highlight fills.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks / tasks / highlights, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
