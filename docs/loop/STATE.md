# DocAgent #1 loop state

Last cycle: 4 (2026-09-06) — byte-identical rerun proof on the landing page

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.

Shipped this cycle:
- CLI `prove`: convert twice (PDF/A + HTML), print both capsules, exit 0 only if bytes and hashes match.
- Test `letter_md_is_byte_identical_across_two_runs` pins `examples/letter.md`.
- README **Same bytes twice** shows the real hashes from that prove (same as `letter-capsule.json`) plus `output-rerun.png`.
- Fixture JSON: `docs/assets/letter-prove.json`. No OmniDocBench / OCR numbers.

Verified hashes (engine 0.1.0, letter.md → PDF/A + HTML):
- input  `2465c17a0efd5023bcd4fe48be2f355c4a09eaf70b1f8b72197b5122545d4e7b`
- plan   `928b47459b089d2dc7e60d6f764b798aaaa955a92ba6977ea6c157e7d88eea63`
- output `74be5c0e8f71cd8162236dfb8802408c34145344636d0c5bec94e08244c0a894`

## Next

1. List markers on the sample letter (markdown `- ` currently strips the bullet).
2. Or PDF drawing of non-table fills beyond the grid.
3. Keep visual density at or above MinerU/Docling for an *agent document runtime*.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
