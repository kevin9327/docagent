# DocAgent #1 loop state

Last cycle: 26 (2026-09-06) — fenced code-block fills

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- IR adds `Paragraph.code_block`. Layout paints a full-width teal background `FillRect` behind the fence.
- Markdown reads/writes ` ``` ` fences. HTML emits `<pre><code>`. DOCX emits `w:pStyle CodeBlock` plus `w:shd` fill `CCFBF1`. ODT emits `text:p` CodeBlock with `fo:background-color="#ccfbf1"`.
- Fixture inserts `docagent prove examples/letter.md` in a fence so the GitHub PDF/HTML screenshots show a real code block.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `7ee000d85cbb55d6049eaa11d52ff34c8ae0da6b637aa24f00056ecbf066f067`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `fc43e2e04bda1dc3909c9be2f9068b0b7cbe63f0a28d64e56ed40ef715e6e382`

Plan hash matches cycle 21–25 (plan is command + targets + fonts, not the paint tree). Input and output hashes changed with the fixture and fence fill.

## Next

1. Hangul codec hyperlinks / strikethrough / quotes / code / breaks, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
