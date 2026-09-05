# ADR 0003 — Canonical length is 1/7200 inch

Status: accepted

The IR stores lengths as `i32` at 1/7200 inch (historically called HWPUNIT, `Hu`).
Twip × 5. Point × 100. PDF points = HU / 100 (only in `docagent-pdf`).
The unit is global; it is not a Hangul-only coordinate system.
