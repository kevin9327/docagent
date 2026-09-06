# DocAgent #1 loop state

Last cycle: 33 (2026-09-06) — table header cells as teal PDF fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Header rows (`row.header` or `header_row_count`) paint `#ccfbf1` cell fills, same teal as code spans.
- HTML `th` uses that fill. DOCX keeps `w:tblHeader` + `w:shd CCFBF1`. ODT keeps `table:table-header-rows` + `THcell`.
- Fixture table already marked the first row as header, so the GitHub PDF/HTML shots now show a teal Hop / File / Proof band.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `927baa86fcdc5d76a72cfbcc8ee32ac3cadbcefb2505bc4bb80b335a14ddef74`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `47f11bf2366c7ce1f629a781814956a0824b415df1bc051f5c9ad91acb70776d`

Input and plan hashes match cycle 32. Output hash changed with the header fills.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks / tasks / highlights / underlines / superscripts / subscripts, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
