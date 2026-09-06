# DocAgent #1 loop state

Last cycle: 22 (2026-09-06) — strikethrough fills and codec roundtrips

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Layout carries `LineFrag.strike`. Paint emits a mid-line `FillRect` (same teal as underlines).
- Markdown reads/writes GFM `~~text~~`. HTML emits `<s>`. DOCX emits `w:strike`. ODT emits `style:text-line-through-style="solid"`.
- Fixture body now ends with `Agents ~~guess~~.` so the GitHub PDF/HTML screenshots show a real strike fill.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `7a1679e9e300ed49159025e7c8aabc0544a504655303f5ac778b6e34d7279526`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `35799beb3bf3c0d9d610b060476801610f2001031c31983dacc3b552356f4caa`

Plan hash matches cycle 21 (plan is command + targets + fonts, not the paint tree). Input and output hashes changed with the fixture and strike objects.

## Next

1. Hangul codec hyperlinks / strikethrough, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
