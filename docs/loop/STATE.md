# DocAgent #1 loop state

Last cycle: 30 (2026-09-06) — font-sized task/bullet markers

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- List markers were 200 HWPUNIT (~2pt) and read as dots on the GitHub PDF screenshot.
- `bullet_disc` is now 0.75em (min 400 HU). Task boxes keep a hollow frame plus a checked inner fill. Advance is disc + 240 HU so text does not overlap.
- Fixture text unchanged. PDF/HTML shots now show readable checkboxes and discs.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `fc05a776bdbb0032bf9f30793238977d5b357ca9b4c777429c19e3c9aa0570d0`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `0c4e99dd5b810ec13631ac472ac79b85108a106c2a8f44759404eb1ead8973f5`

Plan hash matches cycle 21–29. Input hash matches cycle 29 (fixture text unchanged). Output hash changed with the larger marker fills.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks / tasks / highlights / underlines, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
