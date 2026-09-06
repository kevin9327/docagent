# DocAgent #1 loop state

Last cycle: 31 (2026-09-06) — superscripts as a smaller raised face

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- `CharStyle.superscript` lays out at 2/3 size on a baseline raised by 1/3 em.
- Markdown reads/writes `^text^`. HTML emits `<sup>`. DOCX keeps `w:vertAlign superscript`. ODT keeps `Tsuper` `style:text-position="super 58%"`.
- Fixture: `PDF/^A^` in the body, table, nested list, and closer so the GitHub PDF/HTML shots show a raised A.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `66fc8acacb0a2231ab4fd16812bc822e9d88a47f239b0d5ce9099462238e0444`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `c25dd17979888427f4572e1bb08397d0527cc7e8d485d05676fd65e2a7dd82d7`

Plan hash matches cycle 21–30. Input and output hashes changed with the fixture and smaller raised glyphs.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks / tasks / highlights / underlines / superscripts, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
