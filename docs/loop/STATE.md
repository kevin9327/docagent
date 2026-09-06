# DocAgent #1 loop state

Last cycle: 34 (2026-09-06) — hyperlink glyphs as teal PDF text

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Hyperlink runs paint `#0d9488` glyphs (same as HTML `a`), not black text with only an underline.
- PDF uses that fill on `Op::Text`. DOCX keeps `w:color 0D9488`. ODT keeps `Tlink` `fo:color="#0d9488"`.
- Fixture closer `[DocAgent](...)` is teal on the GitHub PDF shot, matching the HTML shot.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `927baa86fcdc5d76a72cfbcc8ee32ac3cadbcefb2505bc4bb80b335a14ddef74`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `86f9fb0ea23d189dddd58705f2330e44312a6b557459d332f9bf188dfe505d92`

Input and plan hashes match cycle 32–33. Output hash changed with the teal glyphs.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks / tasks / highlights / underlines / superscripts / subscripts, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
