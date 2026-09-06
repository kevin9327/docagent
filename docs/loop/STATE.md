# DocAgent #1 loop state

Last cycle: 32 (2026-09-06) — subscripts as a smaller lowered face

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- `CharStyle.subscript` lays out at 2/3 size on a baseline lowered by 1/5 em.
- Markdown reads/writes `~text~` (after `~~strike~~`). HTML emits `<sub>`. DOCX keeps `w:vertAlign subscript`. ODT keeps `Tsub` `style:text-position="sub 58%"`.
- Fixture: `H~2~O` in the body, nested list, and closer so the GitHub PDF/HTML shots show a lowered 2 next to PDF/^A^.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `927baa86fcdc5d76a72cfbcc8ee32ac3cadbcefb2505bc4bb80b335a14ddef74`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `fa51b7d4611202e46c63d21102874259c6167e006c4d7573fc2825deffae2213`

Plan hash matches cycle 21–31. Input and output hashes changed with the fixture and smaller lowered glyphs.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks / tasks / highlights / underlines / superscripts / subscripts, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
