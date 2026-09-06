# DocAgent #1 loop state

Last cycle: 14 (2026-09-06) — ODT heading fo:font-size

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- ODT write emits `Heading1/2/3` paragraph styles with `fo:font-size` 18pt/14pt/12pt and `text:h text:style-name`.
- Read restores run `size` from those styles (and span overlays).
- [`letter.odt`](../assets/letter.odt) from `examples/letter.md` keeps 18pt/14pt headings. PDF/A + HTML hashes unchanged.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `5f33b7f2389a3c4a4f53b638808a8e958305cc8f8a77b976d03f3d41f3d4a85f`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `0fc6d675b4c5a6c773d3da390d7c7e206d0369e232db662fa293c3c415908ccf`

## Next

1. HTML table-cell emphasis (`<strong>`/`<em>` inside `td`/`th`).
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
