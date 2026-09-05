# ADR 0002 — Integer layout

Status: accepted

`dochwp-layout` uses `i32` HWPUNIT and `i64` accumulators. `f32`/`f64` fail CI.
Shaping converts font units with integer multiply/divide.
