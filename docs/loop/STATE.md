# DocAgent #1 loop state

Last cycle: 40 (2026-09-06) — HML FIELDBEGIN Hyperlink on the same IR

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- HML writer emits `FIELDBEGIN Type="Hyperlink"` with `STRINGPARAM Name="Command"` for the URI, display text in `TEXT`, then `FIELDEND`.
- Reader applies the Command URL to the following run as `InlineObject::Hyperlink`.
- First-class letter.md fixture unchanged (input/plan/output hashes match cycle 34–39).

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `927baa86fcdc5d76a72cfbcc8ee32ac3cadbcefb2505bc4bb80b335a14ddef74`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `86f9fb0ea23d189dddd58705f2330e44312a6b557459d332f9bf188dfe505d92`

## Next

1. HWP5/HWP3 hyperlinks, Hangul quotes / code / breaks / tasks, or inline images.
2. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
