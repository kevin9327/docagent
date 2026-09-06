# DocAgent #1 loop state

Last cycle: 23 (2026-09-06) — block-quote left-bar fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- IR carries `Paragraph.quote`. Layout indents the quote and paints a vertical teal bar (`FillRect` 80 HU wide).
- Markdown reads/writes `> text`. HTML emits `<blockquote>`. DOCX emits `w:pStyle Quote` plus a left `w:pBdr`. ODT emits `text:p` style Quote with `fo:border-left`.
- Fixture adds `> Replay is evidence. Same fonts, same bytes.` so the GitHub PDF/HTML screenshots show a real quote bar.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `b513b323320b4aad7f3cf8917a5b844c639bfb38c210417abee0aaa85d29e895`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `68bbe5cf9203b719580cf0a2cc719486b69bfd21a2d045cdf73606638d97798d`

Plan hash matches cycle 21–22 (plan is command + targets + fonts, not the paint tree). Input and output hashes changed with the fixture and quote bar.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes, or inline images, or code spans.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
