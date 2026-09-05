# ADR 0002 — Integer layout

Status: accepted

`docagent-layout` uses `i32` at 1/7200 inch and `i64` accumulators. `f32`/`f64` fail CI.
Shaping converts font units with integer multiply/divide.
