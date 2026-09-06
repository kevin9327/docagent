# Exclusive lanes (max 6)

One owner per path. Never write outside your lane. Never `git commit` or `git push`.
Never edit `crates/docagent-model`, `examples/letter.md`, `AGENTS.md`, or another lane.

| Lane | Write only |
| --- | --- |
| landing | `README.md`, `docs/src/**`, `docs/assets/**` (not `mark.png` once it exists), `docs/loop/competitors.json`, `docs/loop/STATE.md` |
| pdf | `crates/docagent-layout/**`, `crates/docagent-paint/**`, `crates/docagent-pdf/**`, `crates/docagent-raster/**` |
| docx | `crates/docagent-docx/**` |
| odt | `crates/docagent-odt/**` |
| mdhtml | `crates/docagent-md/**`, `crates/docagent-html/**` |
| hangul | `crates/docagent-hwp5/**`, `crates/docagent-hwpx/**`, `crates/docagent-hml/**`, `crates/docagent-hwp3/**` |

`crates/docagent-api`, `crates/docagent-cli`, `crates/docagent-conformance` stay with the orchestrator.

Shared fixture bytes for image tests: `docs/assets/mark.png` (read-only for codec lanes).
