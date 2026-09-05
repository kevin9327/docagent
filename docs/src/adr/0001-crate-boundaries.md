# ADR 0001 — Crate boundaries are the architecture

Status: accepted

Codec crates depend only on `dochwp-model`. Layout does not know formats.
Reverse imports fail in `dochwp-lint` and CI.
