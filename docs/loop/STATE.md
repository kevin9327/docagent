# DocAgent #1 loop state

Last cycle: 20 (2026-09-06) — hyperlinks through codecs

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- Hyperlinks are IR runs (`Run::hyperlink` / `InlineObject::Hyperlink`).
- PDF/A paints underline `FillRect`s. HTML writes `<a href>`. Markdown writes `[text](url)`.
- DOCX writes `w:hyperlink` + `word/_rels` TargetMode External. ODT writes `text:a xlink:href`.
- Fixture last line is `[DocAgent](https://github.com/kevin9327/docagent) runtime — …`.
- Regenerated letter PDF/A, HTML, DOCX, ODT, MD, capsule, prove JSON, and README screenshots.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `64f94fb0ffac49588de2ff9153943bd020205d1a846c06731a9e60d63efea29f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `cd1d971fa71c735c17dd66cab6e4326bdfd3ec5cfbf2513c213172f5a54e70a5`

Plan hash is unchanged vs cycle 19: display glyphs of the last line did not move; the extra paint is the underline fill. Output hash changed with the PDF bytes.

## Next

1. PDF URI `/Link` annotations so the underline is clickable, or Hangul codec hyperlinks, or strikethrough.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
