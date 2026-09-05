# DocAgent #1 loop state

Last cycle: 0 (bootstrap 2026-09-06)

## Public GitHub page (priority 0)

The page strangers see is `https://github.com/kevin9327/docagent`.
It must look like MinerU / Docling / Pandoc: hero, pipeline art, badges,
capability matrix, copy-paste command, real output pictures.

Current:
- Hero SVG + pipeline SVG landed.
- README rewritten to the #1-repo pattern.
- Still missing: photographs of *real* engine output (PDF page, HTML, capsule JSON).

Next public-page work:
1. Convert a sample letter through `docagent` and commit actual PDF/HTML screenshots under `docs/assets/`.
2. Put those images in the README under **What you get**.
3. Keep the first screen (above the fold) denser than the 10 competitors' READMEs.

## Engine

Do not invent OmniDocBench / OCR scores. Win the agent-runtime matrix.
Hangul/rhwp LineSeg is a regional scoreboard, not the GitHub-front north star.

## Rule

Each cycle must ship a visible GitHub-page or engine improvement, then
`git add -A && git commit && git push origin master` when tests allow.
