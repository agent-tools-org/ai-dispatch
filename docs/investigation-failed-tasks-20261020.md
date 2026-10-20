# Failed-task review, 2026-10-15 to 2026-10-20

KB consulted: `read-only violation concurrent commit` (matched
`ai-coding/read-only-agent-is-not-git-read-only.md`: real auditor checkouts exist, so the
guard stays fail-closed for unenforced agents); `hard timeout wrap-up claude tokens` and
`verify offline pending unknown submodule` (no direct match; the remote-bundle submodule
note was a hypothesis the captured diagnostics refuted).

Scope: 37 failed tasks among the last 300 across projects, aid v11.2.0–v11.3.0. Each cluster
below was traced by a read-only investigation against `ec25a4f5`; no tests ran during the review.

## 1. Read-only violations caused by other writers (9 tasks)

`src/cmd/run/read_only.rs` compares the dispatch and settlement snapshots and fails the task on
any difference. The snapshot records HEAD and branch refs, so a commit made in the same checkout
by another process during the audit changes both.

- Six were native Codex runs on `codex app-server`. Each saved rollout records the restrictive
  profile: read access to the root, one writable entry for the task's staging directory,
  approval policy `never`. Four match commits or merges in the checkout's reflog inside the
  run window; two cannot be timed from the reflog but were outside the agent's writable roots.
- Three were Claude runs. Two match merges in the reflog; one is undecidable. Claude's
  read-only mode is advisory (Bash stays available), and one run wrote an ignored file the
  snapshot does not cover.

Decision: a launch whose recorded provenance proves the restrictive native profile reports a
post-run difference as a concurrent-change warning; every other launch keeps failing.
Isolating read-only runs in a HEAD-only worktree was rejected: it drops dirty and untracked
inputs and uninitialized submodules, and does not confine an agent that uses absolute paths.

## 2. Claude tasks at the hard cap (7 tasks)

- No wrap-up was requested. Claude launches with stdin closed (`claude -p <prompt>`), so it
  refuses interactive input; read-only tasks are excluded from wrap-up entirely; and only the
  reaper calls `wrap_up_before_timeout`, never the worker's own deadline.
  Claude Code 2.1.304 supports `--input-format stream-json` and `--replay-user-messages`.
- All seven settled with no tokens or cost. Four never emitted a `result`; three did (up to
  $4.51) but the worker timeout and the reaper settle without reading usage already ingested.
  Claude's `total_cost_usd` is cumulative per process and must not be summed across results.

## 3. Verify failures that were infrastructure (5 tasks)

The shell verify path always sets `infrastructure_failure: false`, so an offline box,
a foreign ensure stamp or a box reset before the command ran is recorded as
`verify_status=failed`. The existing infrastructure outcome is never reached from remote
verify. A box is picked once at dispatch and its auto-picked provenance is lost.

## 4. Lost diagnostics

- Three "Background worker died unexpectedly" causes are undetermined: worker stderr goes to
  `/dev/null` and the reaper overwrites the agent's stderr file with its own line.
- A pending task settled "reason: unknown" after an error between row insertion and prompt
  persistence left no reason.

## Remaining failures

Worktree-lock refusals (3) were correct; three short `FAIL` results and one 60-minute Codex
run were task outcomes, not aid defects.
