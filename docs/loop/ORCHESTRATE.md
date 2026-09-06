# Dispatcher — 15 minutes, fill empty slots, do not wait

You are a **slot filler**, not a six-agent barrier. Finish in a few minutes.
Do not sit until six children complete.

Repo: `C:\Users\swsz9\docagent`
Read `STRATEGY.md`, `LANES.md`, `STATE.md`, `SCORE.md` (if any).

## 1. Hourly score

If `docs/loop/SCORE.md` is missing or older than 60 minutes, rewrite it from
STRATEGY.md (shallow). Then choose this hour’s six tasks from the queue.
Do not invent work.

## 2. Commit whatever is already finished

If the worktree has lane diffs and no other dispatcher is testing:

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --offline`
- If green: one commit, `git push origin master`. Do not wait for empty
  lanes. Update STATE.md.

If tests fail, fix only orchestrator-owned files or revert the broken lane.
Do not start a six-hour debug.

## 3. Fill empty slots immediately (max 6 live)

Slots live in `docs/loop/slots/<lane>.json`:

```json
{"status":"running","task":"...","started":"ISO-8601"}
{"status":"done","task":"...","started":"...","ended":"..."}
```

A slot is **empty** if the file is missing, `status` is `done`, or `status`
is `running` and `started` is older than 20 minutes (stuck).

For each empty lane that has a STRATEGY queue item this hour, spawn **one**
`general-purpose` child (`isolation` none) and write `status: running`.
Spawn all empty lanes in **one** parallel wave, then **return**. Do not
`get_command_or_subagent_output` on them.

If a lane has no queue item, leave it empty. Do not give it trivia.

Children must: write only their lane paths; not commit; not edit model;
clipp + test their crate; on finish, set their slot file to `done`.

## 4. Live cap

Never start a child if six slots already show `running` with fresh
heartbeats. Never spawn two children for the same lane.

## 5. LOCK

Do not use a global LOCK that blocks the whole hour. Per-lane slot files
only.
