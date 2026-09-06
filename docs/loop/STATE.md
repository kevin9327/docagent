# DocAgent #1 loop state

Last cycle: 29 (2026-09-06) — plain underlines as teal PDF fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- `CharStyle.underline` (non-link) paints a teal `FillRect` under the run, distinct from `/Link` annotations.
- Markdown reads/writes `__text__`. HTML emits `<u>`. DOCX keeps `w:u w:val="single"`. ODT keeps `Tunder` `style:text-underline-style="solid"`.
- Fixture: `__09:00 local__` so the GitHub PDF/HTML screenshots show a deadline underline that is not a hyperlink.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `fc05a776bdbb0032bf9f30793238977d5b357ca9b4c777429c19e3c9aa0570d0`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `7ba3af100a56906d0443f16f5246f558de7f02b2b95d6692fbcc25935ddf1d4f`

Plan hash matches cycle 21–28 (plan is command + targets + fonts, not the paint tree). Input and output hashes changed with the fixture and underline fills.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks / tasks / highlights / underlines, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
