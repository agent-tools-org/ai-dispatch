1. FAIL — the leak is not closed globally.

- PASS: `cmd_dispatch` distinguishes `TimedOut`/`InfrastructureFailure` in `src/cmd_dispatch.rs:84-105` and returns exit 1 at `:140-147`. `wait` also checks both at `src/cmd/wait.rs:78-89`.
- PASS: `aid show --json`, board JSON, and the web API expose the raw `verify_status` (`src/cmd/show_json.rs:55-80`, `src/cmd/board.rs:273-290`, `src/web/api.rs:100-143`).
- FAIL: human `show` output reports only `Status: DONE` (`src/cmd/show_output_brief.rs:27`, `src/cmd/show.rs:350-353`); timed-out and infrastructure-inconclusive verification is not shown.
- FAIL: board rendering adds `[VINFRA]` but omits `TimedOut` (`src/board.rs:101-110`). Board counters still count all `Done`/`Merged` tasks as done and exclude inconclusive verification (`src/board.rs:181-187`; `src/cmd/board_stream.rs:202-205,274-279`).
- FAIL: stats, usage, project state, config history, TUI charts, and agent selection count `Done`/`Merged` as success without checking verification (`src/cmd/stats.rs:40-52`, `src/store/queries/task_metrics_queries.rs:73-104`, `src/state.rs:114-121`, `src/usage.rs:368-376`, `src/cmd/config_display.rs:174-211`, `src/tui/charts.rs:103-110`).
- FAIL: MCP `aid_run` and `aid_board` return only `status: done`, with no verification field (`src/cmd/mcp_tools.rs:130-136,281-291`).
- FAIL: watch-stream emits `task_done` and counts zero failures for a `Done` task with inconclusive verification (`src/cmd/watch_stream.rs:106-130`).
- FAIL: group summary and batch outcome treat `Done` as success (`src/cmd/summary_cli.rs:15-22,92-117`, `src/cmd/batch_validate.rs:145-159`).
- FAIL: merge and GitButler lanes accept `Done` without distinguishing inconclusive verification (`src/cmd/merge.rs:57-66,203-205`, `src/cmd/merge_lanes.rs:48-50`).
- FAIL: after-complete hooks and webhooks report a `Done` task as completed/done and do not include `verify_status` (`src/cmd/run_lifecycle.rs:163-182`, `src/cmd/show_json.rs:105-122`, `src/webhook.rs:17-54`).
- FAIL: `RunExitStatus` handles only `Done` specially; a `Merged` inconclusive task falls through to generic “failed” wording (`src/cmd_dispatch.rs:84-124`).
- FAIL: `Pending` is not inconclusive, but `wait` can return success for `Done + Pending` while foreground dispatch returns failure for the same state (`src/types/verify_status.rs:53-55`, `src/cmd/wait.rs:78-89`, `src/cmd_dispatch.rs:140-147`).

2. FAIL — timeout behavior changed beyond the requested infrastructure classification.

`TimedOut` is explicitly documented as distinct from failure (`src/types/verify_status.rs:13-15`), but `is_inconclusive()` now includes it (`:53-55`). It consequently changes exit status and triggers retries.

The retry is bounded: `args.retry == 0` stops it at `src/cmd/run_verify.rs:210`, and the child run decrements the count at `:225-232`; I found no infinite retry loop. However, the change is still behaviorally significant and leaves the task `Done`, prevents failure cascades, and feeds the status-only success consumers above.

3. FAIL — task state and command result are inconsistent.

`enforce_verify_status` still changes a task to failed only for `VerifyStatus::Failed` (`src/verify.rs:192-201`). Infrastructure failure and timeout therefore leave `TaskStatus::Done`, while foreground dispatch and `wait` return exit 1 (`src/cmd_dispatch.rs:140-147`, `src/cmd/wait.rs:78-89`). Lifecycle messaging still recommends merge for `Done` (`src/cmd/run_lifecycle.rs:184-187`).

This could be coherent if every consumer displayed “delivered, verification inconclusive,” but the show, board, MCP, batch, statistics, hooks, webhooks, and TUI paths do not.

Overall: BLOCK

The direct exit/retry fixes are present, but the broader success/failure contract remains incorrect. I did not rerun tests, per instruction.

What did I miss?