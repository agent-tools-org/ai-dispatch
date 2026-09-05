# Task Observation and Control

## Observe

```bash
aid board
aid board --all
aid board --json
aid watch <task-id>
aid watch --tui
aid wait <task-id>
aid show <task-id> --summary
aid show <task-id> --events
aid output <task-id>
aid tree <task-id>
```

`aid board` defaults to the **current project** (the stable identity of the
enclosing git main working tree — `.aid/project.toml` `id` when present). Tasks
dispatched outside any project, and historical rows that never recorded one,
live in the explicit **unattributed** bucket. Use `--all` to list every project;
the CLI keeps its current-project default with --all as the escape hatch.

The TUI shows every project grouped by project, including a visible
**unattributed** group. In the task board, use `j/k` to move within a group,
`h/l` to jump between groups, `Space` to collapse or expand the selected group,
`/` to find a task, `n/N` to move to the next or previous match while finding,
`Enter` to open the selected task, `r` to refresh, and `Esc` to cancel search or
return from a view. `g`/`G` return to the first/last row.

`aid watch --tui` starts with today's tasks plus older active tasks. Press `a`
to toggle all history and `q` to quit. Refreshes run in the background; the
current rows remain usable while the refresh indicator is visible. Repeated
refresh requests are combined, and a refresh failure keeps the last snapshot
visible with an error indicator. Task and tree views render only visible rows.
The multipane view (`m`) loads event histories for its six visible panes.

Use `watch` for a stream and `wait` for automation. `--wait` continues while
verification is pending and returns non-zero when any selected task does not
have a successful outcome. Use `show` to inspect context, diff, output, result,
transcript, audit, event log, agent, model, derived outcome, verification, and
stored completion data. Run `aid show --help` because its modes are mutually
exclusive in some combinations.

`aid show --output` only renders content proven to belong to that task: the
task's recorded working directory (`--dir` after worktree remapping), its
worktree, or `~/.aid/tasks/<id>/`, or the persisted `result.md` under that task
dir. Tasks created before that directory was stored on the row recover an
absolute `--dir` from their persisted dispatch args. Rows with no usable
recorded directory stay empty and report absence. Relative `-o` paths are never
read from the caller's CWD or from the shared repository root (either would
leak another task's report). Paths that escape the chosen base via `..` or a
symlink are rejected. When the declared output is missing, the absence is
stated explicitly and the fallback is this task's own log.

Human task surfaces use verification tags only when verification has something
to say: `VFAIL` for a failed verification, `VTIMEOUT` for a timeout, `VINFRA`
for a verification infrastructure failure, and `VNORESULT` when a required
verification has no result. Running tasks and tasks that skipped verification
without that case have no tag. The tags appear in
board rows, `aid show`, the TUI, and task detail.

## Communicate with a live task

```bash
aid respond <task-id> "Answer to the pending question"
aid reply <task-id> "Report current blocker"
aid steer <task-id> "Keep the API unchanged; update only validation"
aid unstick <task-id>
```

- Use `respond` when the task is explicitly awaiting input.
- Use `reply` for a tracked message with acknowledgement behavior.
- Use `steer` for updated direction during execution.
- `steer` is refused for the one-shot print-mode `agy` and `grok` CLIs because
  they do not consume PTY stdin; aid reports the limitation before queuing a
  steer message. Codex steering remains supported.
- `respond` is refused for those same one-shot CLIs before aid writes a
  response signal; aid says that no response signal was written. Use it only
  when the selected adapter consumes PTY input.
- Use `unstick` when progress has stopped and recovery is appropriate.

Do not send repeated polling messages; inspect events first.

## Automatic safeguards

AID enforces configured idle, hung-task, cost, and maximum-duration safeguards.

A task that has produced zero progress since spawn is reaped on the shorter
first-token budget (default 180s, `AID_FIRST_TOKEN_TIMEOUT_SECS`). For PTY
agents this is the first-token dead-stream detector; for buffered background
agents (grok, agy) the background reaper applies the same distinction — silence
since spawn versus silence after progress. Only the latter waits out the full
idle margin.

`--idle-timeout SECS` stops a task whose stream goes quiet. Meaningful text
output refreshes the liveness clock even when aid cannot parse it into an event
(a Grok/agy-style CLI, for example), so unparseable output does not read as
silence. This signal only sees what the agent emits: silent work is not
observable by it. Pure terminal-control noise (spinners, cursor hides), idle
auto-nudges, their PTY echoes, and aid's own reply/ack bookkeeping do not count
as agent progress, so a genuinely stalled or silent agent is still reaped after
the idle window.

`--timeout SECS` is activity-aware rather than a hard wall-clock cap: an active
foreground run may continue past it, and the value is rounded up to whole
minutes.

Repeated activity is not itself a stop condition.

## Stop and retry

```bash
aid stop <task-id>
aid stop <task-id> --retry-tree
aid retry <task-id> --feedback "Fix the failing test" --bg
aid retry <task-id> --feedback "try again" --model gpt-5.4
aid retry <task-id> --feedback "try again" --idle-timeout 900
aid retry <task-id> --feedback-file notes.md
```

A retry replays the directory the original run actually used, not the one you
happen to be standing in when you type the command. A task dispatched without
`--dir` records the absolute directory it ran in, and the retry resolves to
that, falling back to the task's repository and refusing when neither is
usable. This matters for agents that key their saved sessions by working
directory: without it, resuming a session from a different directory fails
immediately.

Stopping preserves the worktree and attempts to preserve in-flight changes.
Inspect the artifact afterward. A retry creates linked history; use `tree` to
understand the chain.

`aid retry <task-id>` on a non-terminal task supersedes that task's own run: aid
stops the still-live worker first, then starts the new attempt in the same
worktree. If the worker cannot be stopped, the retry is refused. A retry still
refuses a worktree genuinely held by a different live task.

Unspecified `--model` and `--idle-timeout` inherit the original task's saved
values (not global defaults). `--feedback` and `--feedback-file` (`-F`) are
mutually exclusive; provide exactly one.

When the recorded linked worktree still exists, retry reuses it with the
original repository checkout as its anchor. Retry still refuses a target branch
that is genuinely checked out in the checkout that dispatched the task.

### Foreground signal behavior

Foreground aid run and aid retry dispatch the same detached worker used by
--bg, then attach a watcher that reports progress and waits for the real
terminal outcome. The worker is double-forked and reparented before the
foreground process can be killed by a caller's process-tree timeout; its PTY or
pipe output is written to the task log rather than held by the watcher.

- **Interactive stdin, SIGINT/Ctrl-C**: stops the task and records Stopped.
- **Interactive stdin, SIGTERM/SIGHUP**: preserves the existing stop behavior.
- **Non-interactive stdin, SIGTERM/SIGHUP**: leaves the task running, prints
  aid watch --wait <task-id>, and exits with the signal status.
- **SIGINT without a TTY**: still stops because it means interrupt, not timeout.

PTY agents (opencode, mimocode, and kilo) continue through pty_runner in the
worker. Foreground and background runs therefore share the same agent,
timeout, session-resume, cost, verification, retry, audit, and delivery
lifecycle.

Persisted job specs may contain fields removed by a newer aid version; unknown
fields are ignored so board, stop, and unstick remain usable during upgrades.
The reaper warns and skips a spec it cannot read, allowing cleanup of other
tasks to continue.

A worker can also die *after* clearing its background spec — the spec is
written at dispatch and removed by a guard when the worker exits, so a process
killed between those points, or one whose terminal write failed, leaves a
`Running` row with no spec behind it. Such a row is now judged by the same
liveness signals as any other: task events and the agent log's own bytes. Once
it goes quiet for its idle window the reaper records `hung detected (orphaned
supervisor)`; past 24 hours the maximum-runtime path claims it instead and
records `exceeded maximum runtime`. The two reasons are distinct on purpose —
silence is not the same fact as a run that outlived its cap.

A live foreground watcher is attached to the same worker spec as `--bg`, and
the reaper sees the worker PID independently of the watcher. Only a task whose
worker is already gone can reach the spec-less path. Such a row reads `RUN`
with a growing timer until the 24-hour cap — a task dead for fourteen hours
still displayed as running.

## Merge

```bash
aid merge <task-id> --check
aid merge --group <group-id> --approve
```

Merge integrates delivered code and records `Merged`. By default it requires a
successful outcome; a failed or inconclusive verification is refused. This
gate applies to single-task merges, group merges, and GitButler lane merges.
`--force` is the explicit override and records why the verification gate was
overridden. It does not accept the delivery on behalf of the principal and does
not authorize worktree deletion. Review can happen before or after integration
depending on team policy, but `aid accept` must remain an explicit principal
decision.

For GitButler lanes:

```bash
aid merge --group <group-id> --lanes
```

Review applied lanes with GitButler. Worktrees remain in custody.

## Export and reports

```bash
aid export <task-id> --format md --output task.md
aid usage --period 7d
aid cost --summary
aid stats --window 7d
aid notifications
aid changelog
```

Use exports for sharing, not as a substitute for the stored task record.

`aid stats` reports success from `TaskOutcome`, so `Verified` and `Delivered`
are the only successes and a verification failure is a failure even when the
task was delivered. A `Done` task with `hollow_output` or
`missing_final_delivery` is judged not successful, so empty agent runs do not
inflate success rates used by `aid advise`. `aid stats` and `aid cost` show
`unknown` when a task's model has no known pricing. Unknown costs are omitted
from totals rather than recorded as `$0.00`.

## Machine-facing completion data

MCP task views, task hook payloads, and task webhooks add `outcome` and
`verify_status` fields. Existing `status` fields retain their lifecycle meaning;
consumers must use `outcome` when deciding whether work succeeded. The values
are additive and use the stored verification names, including `pending`,
`timed_out`, and `infrastructure_failure`.

## Model attribution: requested versus observed

A task records two models, never one:

- `requested_model` — what aid dispatched with, from `--model`, the configured
  default, budget mode, or smart routing. It is a request, and it is kept even
  when the CLI refused to serve it.
- `observed_model` — what the CLI reported it actually ran. It is `null` when
  the CLI reported nothing, which is **not** the same as the requested model
  having run.

A third field, `attribution_source`, records how `observed_model` was
established. It moves with it and is `null` whenever the model is:

| Value | Meaning |
|---|---|
| `echoed` | The CLI named the model in its own output. The strongest evidence available. |
| `confirmed_by_success` | aid passed an explicit model and the run succeeded, so that model ran — a CLI handed a model it cannot serve fails instead. Inferred from the absence of a refusal, not from a statement. |

`aid show --json`, `aid board --json`, the MCP task view, and the web API emit
all three fields. There is no single `model` field.

Human surfaces render them together: `gpt-5.6` when request and observation
agree and the CLI said so, `gpt-5.6 (inferred)` when the model was confirmed by
the run succeeding rather than stated, `gpt-5.6?` when a request was never
confirmed at all, and `composer-2 (asked auto)` when the CLI served something
other than what was asked for.

A router alias such as `auto` is never confirmed by success: it selects a model
rather than being one, and confirming it would put a router back in the model
column.

`aid stats`'s per-model breakdown reads `observed_model` at either grade. An
agent's learned default model accepts `echoed` only, because a model inferred
from a run not failing is not evidence that model performed well — and a CLI
that silently substitutes on success would defeat the inference. Cost estimation falls back to `requested_model` when
there is no observation; because both fields are stored, a reader can tell which
basis a given row used. Per-family quota marking also reads the request on
purpose — it asks which family aid aimed at, and plain-text CLIs never echo a
model at all.

Several CLIs never report a model, codex and agy among them, so `unknown` is the
honest and expected value for many tasks rather than a defect.

When a task has no assistant transcript, `aid export --sharegpt` falls back to
the recorded events, and a failed task's salvaged `partial-work.md` summarises
its recent activity. Both preserve tool calls, file reads, and file writes, so a
task that only edited or only read files is still represented rather than
reported as having recorded nothing. Reasoning narration is not treated as tool
activity in either place.

## Recovery rules

1. Inspect `aid show <task> --events` and the worktree before acting.
2. Preserve failed, stopped, hollow-output, and missing-delivery artifacts.
3. Do not repair stale Git metadata with `git worktree prune`.
4. Do not force-reset an AID branch to make reuse succeed.
5. Resolve ownership and review first; use acceptance and custody GC only at
   the end of the lifecycle.
6. Use `aid doctor` for evidence; `aid doctor --apply` may safely rewrite leaked operator symlinks, but does not delete task artifacts.
