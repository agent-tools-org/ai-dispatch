# Task settlement: one terminal status

Status: design, not implemented (2026-09-30, v10.50.0).

## Problem

Agent runners publish a terminal task status when the agent process exits
(`src/pty_runner.rs`, `src/cmd/run/process.rs`, `src/cmd/run/agent/timeout.rs`).
Settlement then runs in the worker (`src/background.rs` → `run_post_lifecycle`):
dirty-worktree rescue, verification, result-file checks, quota rescue, retries,
hooks and backup. Settlement can change the published status (Done → Failed on
verification or a missing report; Failed → Done on quota rescue).

Readers compensate differently:

- `aid wait` treats an existing job file as "still settling" and has a separate
  verify-in-flight predicate.
- Foreground attach checks terminal status, verify status, retries and the job file.
- Batch dependencies, `aid accept`, merge, watch and the Web SSE stream read terminal
  status alone, so they can act on a status that settlement later changes.
- The reaper only considers Running tasks, so a worker that dies during settlement
  leaves a provisional status and an orphan job file.

The settlement E2E harness (`tests/settlement_e2e.rs`) records the observable cases
as ignored scenarios.

## Target

- At agent exit the runner records run facts (tokens, cost, duration, observed model,
  exit code) and an `agent_exited_at` timestamp; status stays Running.
- Settlement carries a provisional outcome in memory; rescue, verification, result
  checks and quota rescue change only that value.
- One `publish_terminal_status` runs after continuation selection. Notifications,
  hooks, webhooks, backup and continuation dispatch follow it; the job file is then
  removed and again means only "worker alive".
- No new task status. "Settling" = Running with `agent_exited_at` set and a live worker.

Consequences:

- Reaper: settling tasks skip idle checks; the hard cap still applies; a dead worker
  while settling takes the zombie path ("worker died during settlement").
- `aid stop` while settling kills the worker group; a later publish is rejected by the
  existing completion guard.
- `aid wait <parent>` follows retries like foreground attach.
- `duration_ms` stays agent runtime; `completed_at` is the publish time.
- Hooks and webhooks fire once, with the final status.

## Steps (each independently mergeable)

| Step | Change | No-behaviour-change? |
| --- | --- | --- |
| F1 | Split the completion write into `record_agent_exit` + `publish_terminal_status`; runners still publish immediately | yes |
| F2 | A settlement value threaded through the lifecycle; writers, then readers, go through it while the database is still mirrored | yes |
| F3 | Announce once: notify, hooks, webhooks after continuation selection | timing only |
| F4 | Continuation decided, then published, then dispatched in the background | yes for results |
| F5a | Settling-aware reaper and stop (inert until F5b) | yes |
| F5b | Runners stop publishing; the worker publishes once; ignored harness scenarios enabled | the change |
| F6 | Remove reader compensations (`wait_settlement`, verify-in-flight checks); one release after F5b so workers started by an older binary settle first | cleanup |

Worktree observation work (one status capture per settlement step) must land before
F2, because both edit dirty-worktree settlement.
