# Exclusive lanes (max 6 live)

One owner per path. Never write outside your lane. Never `git commit` or `git push`.
Never edit `crates/docagent-model` or `AGENTS.md`.

| Lane | Write only |
| --- | --- |
| landing | `README.md`, `docs/src/**`, `docs/assets/**` (not `mark.png`), `docs/loop/competitors.json`, `docs/loop/STATE.md`, `docs/loop/SCORE.md` |
| pdf | `crates/docagent-layout/**`, `crates/docagent-paint/**`, `crates/docagent-pdf/**`, `crates/docagent-raster/**` |
| docx | `crates/docagent-docx/**` |
| odt | `crates/docagent-odt/**` |
| mdhtml | `crates/docagent-md/**`, `crates/docagent-html/**` |
| demo | `examples/letter.md`, `docs/assets/letter.pdf`, `docs/assets/letter.html`, `docs/assets/letter.docx`, `docs/assets/letter.odt`, `docs/assets/letter-capsule.json`, `docs/assets/letter-prove.json`, `docs/assets/output-*.png`, `crates/docagent-cli/**`, `crates/docagent-api/**` (tests/prove only) |

Hangul crates are **not** a standing lane. Only touch them if SCORE.md
names Hangul as the reason we lose a first-class axis.

`docs/assets/mark.png` is read-only fixture bytes.
