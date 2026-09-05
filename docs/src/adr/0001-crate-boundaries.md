# ADR 0001 — Crate boundaries are the architecture

Status: accepted

Codec crates depend only on `docagent-model`. Layout does not know formats.
Reverse imports fail in `docagent-lint` and CI.
