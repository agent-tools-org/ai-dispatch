# Investigation — failed tasks, 2026-10-10 15:00 to 2026-10-13 15:30 (+07:00)

KB consulted: `kb model capacity worker died read-only violation hung` — related notes
`failover-granularity-must-match-metering`, `a-running-status-is-not-a-running-agent`,
`agent-dispatch-hygiene`; none covers these clusters directly.

Method: `aid board --all --json`, `aid show <id> --events` / `--log --full` for every
failed, stopped or flagged task, job files under `~/.aid/jobs/`, and two read-only code
traces against `9062bd08` (v10.52.1). Every classification cites an event line or a
`file:line`. Durations come from events, because the board reports `0 s` for 16 of the
rows (see §5).

## Scope

395 tasks across 6 repositories.
Codex ran 393 of them. Result: 331 done, 56 failed, 1 stopped, 7 still running; 3 done
tasks carry `delivery_assessment: empty_diff`.

## Verdict

| # | Cluster | Tasks | Agent time lost | Ours? |
|---|---------|-------|-----------------|-------|
| 1 | Codex "Selected model is at capacity" | 23 | ~350 min | cause: provider; handling: **aid gap** |
| 2 | Hard timeout (`--timeout`) reached | 11 | ~14 h of runs ended without a report | mixed: dispatch budgets + **aid gaps** |
| 3 | Idle watchdog kills a task waiting on a silent child | 7 | ~455 min | **aid bug** (roadmap item, unfixed) |
| 4 | Read-only audit wrote files | 7 (+1 also in #1) | 0 (reports exist) | agent fault; **aid gap** in enforcement |
| 5 | `aid stop` recorded as a worker crash | 4 | 0 | **aid bug** |
| 6 | Quota / auth refusal at launch | 4 | < 1 min | partly **aid gaps** in detection |
| 7 | `empty_diff` on capture/research tasks | 3 (done) | 0 | cosmetic **aid gap** |

Cross-cutting **aid bug**: failed and stopped rows written by the reaper or by `aid stop`
keep `duration_ms` and `completed_at` NULL (§5).

---

## 1. Model capacity — 23 tasks

Codex route `openai-chatgpt-plan/gpt-6.1-sol` ends with
`Selected model is at capacity. Please try a different model.`, usually after
`Reconnecting... n/5 (unexpected status 503 Service Unavailable ...)`. Clusters:
10-12 15:11–15:27, 10-12 20:30–21:14, 10-13 13:09–15:05. Several tasks had passed
their checks and were writing the report (t-25fe4947 "Completed web metadata, design
docs and regression tests"; t-27488806 "Focused verification passed").

Tasks: t-7577fa79 t-663d25d3 t-79442077 t-6a736f36 t-6c8f1332 t-0aa4fc43 t-5388dcee
t-3d556961 t-3e7fb8c9 t-a252039b t-25fe4947 t-fd58cd82 t-d5fda04f t-27488806 t-b6f55df9
t-274c829a t-07587280 t-771258e2 t-f14d3c4e t-70ad1553 t-8dec695a t-ce25bf2a t-9b2f61a0.

What aid does today:
- No classifier matches "at capacity" or 503 (`agent/codex/events.rs:157-178` →
  `rate_limit.rs:501-520`; signatures `rate_limit_signatures.rs:83`, generic tokens
  `rate_limit.rs:463-469`; `model_health.rs:8-22` has no capacity pattern). No hold, no
  quota cascade, no model self-heal.
- Failure salvage works: `failure_salvage.rs:28-49` commits `wip: partial work salvage`.
  Each continuation was a new hand-written dispatch ("… cut off by model capacity.
  Continue from the wip commit") — e.g. t-5388dcee → t-79442077 → t-7577fa79, three cut-offs in a row.

Gap: a transient provider capacity error is handled like an agent failure. Continuing
from the salvage commit with the same brief is mechanical, yet needs a new dispatch.

## 2. Hard timeout — 11 tasks

`Task exceeded max duration (3601s > 3600s)` (or 10801s > 10800s):
t-82417da1 t-6cb9380d t-a83c5189 t-95eabb11 t-36e10187 t-326858fe t-61c1d905 t-ffe9b058
t-14701f36 t-8423bec6, plus t-6813ed64 (`91s > 90s`).

- Most runs spent their last 20–40 minutes waiting on remote build-box checks (t-61c1d905, t-a83c5189, t-36e10187, t-ffe9b058).
- t-95eabb11 logged "Final verification complete; source-only branch is clean" 77 s
  before the kill; t-14701f36 had "All final iOS unit tests … passed" 26 min before.
  The work exists on the branch; the task is recorded as failed with no report.
- t-6813ed64: the job file has `max_duration_secs: 90`. `--timeout` takes seconds
  (`aid run --help`); the brief budgeted 90 minutes. Dispatch error, but aid accepted a
  90-second cap for an iOS fix with simulator tests without comment.

Gaps: no warning before the cap (the agent cannot wrap up), no plausibility check on the
value, and the terminal row has no duration (§5).

## 3. Idle watchdog kills a waiting task — 7 tasks

`Agent hung: no output for 600 seconds`: t-eefd85c5 t-ce06348a t-1c4826d8 t-9175a79e
t-d96ab669 t-9013f193 t-02d3463f (6 of them iOS/macOS builds and simulator tests, ~455 min of runs).

Activity is agent output only (`timeout_policy.rs:11`, `watcher.rs:83-99`,
`pty_watch.rs:492-515`, `background_orphan.rs:59-79`). Codex prints `item.started` and
nothing else until a command finishes, so a silent `xcodebuild test` or remote wait of
more than 600 s reads as a hang. t-1c4826d8 had committed `2343630a` and passed
`security-guard` before it was killed. Idle hangs retry only with `--retry`
(`cmd/run/post_hung.rs:51-66`). t-02d3463f is different: codex itself stalled after
`workspace routing discovery timed out` reconnects.

The roadmap already lists "Idle timeout during box waits" as not started.

## 4. Read-only violation — 7 tasks (+ t-274c829a)

Audits that exited 0 with a finished report: t-3c7d5967 t-461e6620 t-4ff078e4 t-03ca3b10
t-49f7afb1 t-5800d8de t-919336df. Each wrote regression tests or investigation notes
into the checkout (e.g. t-49f7afb1 touched five source and test files).

- Enforcement is after the fact: `cmd/run/read_only.rs:33-61` marks Done → Failed.
  Codex launches with the same write sandbox as a write task plus a prompt prefix
  (`agent/codex.rs:71-73`, `agent/read_only.rs:7-26`, `agent/codex/capabilities.rs:19-33`);
  claude, grok, commandcode and droid use real read-only modes.
- The report survives at `<task_dir>/result.md` (`lifecycle/phases.rs:49-53`), but
  `--audit` post-processing is skipped, and with `--verify` set aid adds
  "Configured verification did not run: dirty worktree settlement failed before verify",
  which reads like a second failure.

## 5. `aid stop` recorded as a crash; NULL durations — aid bugs

t-13b56270 t-3864b071 t-4aae68bf t-3cff02e5 each end with, in the same second:
`Background worker died unexpectedly` → `rejected illegal status transition: failed ->
stopped` → `Task stopped by user`. Final status `failed`.

`aid stop` (`cmd/stop.rs:80-131`) kills the worker, waits up to 5 s, preserves the
worktree, and only then calls `mark_stopped`. Any concurrent `aid` process (wait, watch,
board, MCP) runs the reaper (`background_reaper.rs:73-119`, called from `main.rs:178`
and polling loops), sees a dead pid on a `running` row, and records a failure first.
`Failed → Stopped` is illegal (`types/status.rs:104`); `stop.rs:117` ignores the `false`
return and still logs "Task stopped by user".

`duration_ms` and `completed_at` are written only by the normal completion path
(`store/mutations.rs:379-450`), which refuses failed/stopped rows. The reaper paths (max
duration, worker death, hard cap) and `mark_stopped` update `status` only, so 16 rows in
this window have no duration and the run summary prints `0s` (`cmd_dispatch.rs:68-73`).

## 6. Quota / auth refusals — 4 tasks

| Task | Agent | Line | Detection |
|------|-------|------|-----------|
| t-d7099256 | claude | `rate_limit_event` `status:"rejected"`, `seven_day` | detected, held to reset (`rate_limit_claude.rs:68-76`); no `seven_day` fixture |
| t-acb810ac, t-b0328ae8 | droid | `402 … monthly Droid Core usage limit (resets in 2 days)` | generic `402` only; no "monthly" signature (`rate_limit_signatures.rs:80`); held as group `core` |
| t-3902c27c | opencode | `APIError 401 … Authentication error` (code 10000) | **not detected**: no auth signature for opencode (`auth_marker.rs:18-26`) |

t-b0328ae8 launched droid 18 h after the first refusal while the `core` hold was active.
A default-model droid run resolves to the `standard` group (`agent/model_group.rs:108`),
so the hold did not apply, but the account refused on Core. Whether this account's
default model draws on Core is **unverified**.

## 7. `empty_diff` on capture tasks — 3 done tasks

t-a4006989 (197 screenshots), t-5f11b036, t-2eb2a077 delivered files outside the
repository by design. `cmd/run/post.rs:62-91` flags any non-read-only worktree task with
an empty git diff; there is no "no repository change expected" notion. Informational,
but noise in `board --json`.

---

## Work items for the next release

Ordered by state correctness first, then lost agent time.

| Order | Item | Acceptance contract |
|-------|------|---------------------|
| 1 | Stop wins over the reaper | `aid stop` racing `aid wait` ends `stopped` with no crash event; stop intent is recorded before the signal |
| 2 | Terminal rows carry duration | Every failed/stopped row has `completed_at` and `duration_ms` (from `started_at`, else `created_at`); board and run summary never print `0s` for a run that lasted minutes |
| 3 | Live child is activity | A codex task inside a silent 15-minute command is not killed as idle while a descendant process accrues CPU; the hard cap still applies |
| 4 | Capacity is transient, not failure | "at capacity" / exhausted 503 reconnects classify as transient capacity: a short model-level hold and one automatic continuation from the salvage commit (codex session resume) that does not consume `--retry`; repeated capacity then cascades per advise |
| 5 | Wrap-up before the hard cap | At 90 % of `--timeout` the agent is steered to commit and report; `--timeout` below 300 s warns that the unit is seconds |
| 6 | Codex read-only at launch | `--read-only` codex runs in a read-only sandbox with only the task directory writable (needs roadmap slice "Read-only report location"); a violation no longer adds a "verification did not run" error |
| 7 | Refusal signatures | opencode 401 sets an auth marker; droid "monthly … usage limit" is a dated hold; a `seven_day` Claude fixture exists |
| 8 | Capture tasks | A task that declares no repository change is not flagged `empty_diff` |

Items 1–3 are bug fixes and need a failing regression test plus an independent audit
before release. Items 1 and 3 overlap the roadmap's "Task settlement" and "Idle timeout
during box waits" slices; land them there rather than as new layers.

Outside aid: provider capacity itself, agents writing files during read-only runs, and
caller-chosen `--timeout` values shorter than the task's remote checks.
