# Triage: held-route substitution that ends as `skipped`

Date: 2026-08-09

Scope: read-only trace of `t-402001d3`, `t-09bf6de1`, and `t-d93c7ccb`. No
source behavior was changed. The report itself is the only artifact added.

## 1. Where substitution happens, and why the task is skipped

The held-route substitution is implemented in `src/cmd/run_dispatch_resolve.rs`
and `src/cmd/run_dispatch_resolve_held.rs`:

1. `resolve_agent_setup` checks the requested route with
   `rate_limit::dispatch_blocking_hold` at `src/cmd/run_dispatch_resolve.rs:147-149`.
   For the held `codex` route it calls `skip_held_to_fallback` at
   `src/cmd/run_dispatch_resolve.rs:154-155`.
2. `skip_held_to_fallback` walks the explicit cascade and then the automatic
   coding fallback, returning the first candidate without a blocking hold at
   `src/cmd/run_dispatch_resolve_held.rs:38-60`.
3. The caller logs the warning, switches `args.agent_name`, replaces the
   effective agent kind, and records `(original, hold)` at
   `src/cmd/run_dispatch_resolve.rs:156-164`. That is the implementation of
   the substitution itself.
4. `prepare_dispatch` inserts the task and then inserts the substitution
   milestone at `src/cmd/run_dispatch_prepare.rs:94-95`. The exact text is
   assembled at `src/cmd/run_dispatch_resolve_held.rs:64-82`; its
   `format!` at lines 76-80 produces:

   ```text
   Held route skipped: codex (...) — dispatching to claude instead. ...
   ```

5. The task's dispatch arguments, including the post-substitution agent and
   `dry_run` flag, are persisted at `src/cmd/run_dispatch_prepare.rs:121-124`.
6. `aid run` assembles the prompt and then branches on `args.dry_run` at
   `src/cmd/run_dispatch.rs:32-45`. The dry-run helper deliberately calls
   `mark_skipped` at `src/cmd/run_dispatch.rs:137-158`, prints dry-run
   information, and returns without reaching the binary check, running-state
   transition, or agent process.

The three database rows confirm this exact path:

| Task | Persisted agent | Status | `dispatch_args.dry_run` | Events |
|---|---|---|---:|---|
| `t-09bf6de1` | `claude` | `skipped` | `true` | substitution milestone only |
| `t-402001d3` | `claude` | `skipped` | `true` | substitution milestone only |
| `t-d93c7ccb` | `agy` | `failed` | `false` | substitution milestone, then SIGTERM error |

Therefore the “announced substitute never happened” result is not a failed
Claude dispatch. The caller requested a dry run, and the milestone wording
does not say that the substitute is only hypothetical.

## 2. Why AgY ran and Claude did not

The difference is `dry_run`, not the agent:

- With `dry_run=false`, `run` continues after line 45 through the reset wait,
  binary check, workspace setup, `mark_running`, and foreground/background
  execution at `src/cmd/run_dispatch.rs:47-95`. That is the AgY row's path;
  its later SIGTERM event is consistent with the process having run.
- With `dry_run=true`, execution returns at `src/cmd/run_dispatch.rs:44-45`
  before any agent command can be spawned. The preflight also explicitly skips
  the PATH probe for dry runs at `src/cmd/run_validate.rs:50-58`.

There is no Claude-specific silent return:

- `ClaudeAgent::build_command` only constructs `claude -p ...` and returns a
  `Command` or an error at `src/agent/claude.rs:24-53`.
- Claude is a normal built-in binary candidate at `src/agent/binary.rs:75-92`,
  including `claude` at line 89.
- Caller detection only records metadata in `src/cmd/run_dispatch_prepare.rs:81`
  and `src/cmd/run_dispatch_prepare.rs:133-153`; it does not reject a task
  whose caller is `claude-code`.
- The nested-dispatch guard is generic `AID_TASK_ID`/depth handling at
  `src/cmd/run_delegation.rs:15-40`; it has no Claude branch.
- If a non-dry-run Claude binary is missing, the generic preflight marks the
  claimed task `failed` and writes an error event at
  `src/cmd/run_dispatch.rs:98-133`. It does not silently mark it `skipped`.

## 3. Whether `skipped` is intended and what callers report

`Skipped` is intentional for an explicit dry run: the helper documents that
the task was deliberately not executed and uses `Skipped` as its terminal
state at `src/cmd/run_dispatch.rs:143-158`. It is not the error state for a
substitution that failed to run.

Other failure paths preserve that distinction. If no fallback can be selected,
`skip_held_to_fallback` returns an error at
`src/cmd/run_dispatch_resolve_held.rs:53-60`, before the task is claimed. If
worktree setup fails after claiming, `fail_claimed_task` records `failed` plus
an error event at `src/cmd/run_dispatch_prepare.rs:263-275`. A missing binary
also records `failed`, as cited above.

User-visible behavior for these rows is:

- `aid run`: `DispatchOutcome::run_exit_status` returns no status for either a
  background or dry run at `src/cmd_dispatch.rs:42-49`. Consequently `main`
  has no run summary or nonzero override to apply at `src/main.rs:149-154`;
  this dry-run command completes with the normal process exit code 0 after the
  dry-run lines are printed.
- `aid board`: skipped rows render as `SKIP`, with duration, tokens, and cost
  shown as `-` at `src/board.rs:110-128`. The ordinary board status text does
  not append the milestone to a skipped row because milestone enrichment is
  limited to `RUN` at `src/board.rs:254-268`. Its top-level status counts also
  omit skipped from done/running/failed at `src/board.rs:176-188`.
- `aid watch --wait`: the `--wait` route calls `cmd::wait::run` at
  `src/cmd_dispatch/display.rs:83-86`. `wait` treats every terminal status,
  including `Skipped`, as completed at `src/cmd/wait.rs:126-150` and exits
  successfully once no active tasks remain at `src/cmd/wait.rs:172-188`.
  It prints the terminal row as `SKIP` with `-` duration/tokens/cost and no
  failure reason. The stream variant names this terminal event
  `task_skipped` at `src/cmd/watch_stream.rs:134-146`.

## Proposed minimal fix

Keep the existing explicit dry-run semantics, but make the substitution
milestone dry-run-aware: pass `args.dry_run` into the milestone helper and use
wording such as `dry-run: would dispatch to claude instead` when true. That
would prevent the current message from claiming that dispatch is occurring
while preserving the useful `Skipped` terminal state.

For the three reported tasks, the immediate operational fix is to remove the
unintended `--dry-run` at the caller. No Claude adapter, self-dispatch guard,
caller-pool guard, or binary fallback change is warranted by this trace.

what did I miss?
