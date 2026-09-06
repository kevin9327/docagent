# DocAgent #1 loop state

Last cycle: 24 (2026-09-06) — code-span background fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- IR carries `CharStyle.code`. Layout paints a teal background `FillRect` behind the run.
- Markdown reads/writes `` `text` ``. HTML emits `<code>`. DOCX emits `w:rStyle Code` plus `w:shd` fill `CCFBF1`. ODT emits `Tcode` with `fo:background-color="#ccfbf1"`.
- Fixture body now says `Run \`prove\` twice` so the GitHub PDF/HTML screenshots show a real code span.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `7eefb2dde905c384cc1d7f1a4521532f5cffad2798d17cf8414c16ac814b5420`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `9eee08710a754fa8b9c3e47e31e9875d17260a96e55f387b540e82e59ad30a0c`

Plan hash matches cycle 21–23 (plan is command + targets + fonts, not the paint tree). Input and output hashes changed with the fixture and code fill.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
