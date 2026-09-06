# Dispatcher — always six live (no 15-minute wait)

You are a **slot filler**. Finish in under two minutes. Never wait for children.

Repo: `C:\Users\swsz9\docagent`
Read `STRATEGY.md`, `LANES.md`, `SCORE.md`.

## Always six — 1 out, 1 in

There must be **six** `status: running` slots after you return.
A slot is empty if missing, `done`, or `running` older than 12 minutes.

Write the slot file `status: running` with `started` = now (UTC) **first**,
then spawn. Re-read every slot file immediately before spawning — if it
is already `running` and younger than 12 minutes, skip that lane (another
filler got there). Never two children on one lane.

Spawn every remaining empty lane in one parallel wave, then **return**.
Do not `get_command_or_subagent_output`.

Refill is **immediate**. The 60s scheduler is a safety net only — never
wait for the next fire if a slot is already empty in this turn.

Never leave a lane idle because “the feature already shipped”. Use that
lane’s **standing #1 job** from SCORE.md.

Never two children on one lane. Cap 6 running.

## Hourly score

If `SCORE.md` is missing or older than 60 minutes, rewrite it shallowly vs
the ten. Still fill six slots the same firing.

## Commit

If the worktree has diffs: clippy `-D warnings`, `cargo test --workspace
--offline`, one commit, push. Do not wait for other lanes.

## Children

Write only their lane. Do not commit. Set their slot file to `done` when
finished. No Hangul trivia, no wrap-mode archaeology, no new IR, no OCR.
