# Task Observation and Control

## Observe

```bash
aid board
aid board --json
aid watch <task-id>
aid wait <task-id>
aid show <task-id> --summary
aid show <task-id> --events
aid output <task-id>
aid tree <task-id>
```

Use `watch` for a stream and `wait` for automation. `--wait` continues while
verification is pending and returns non-zero when any selected task does not
have a successful outcome. Use `show` to inspect context, diff, output, result,
transcript, audit, event log, agent, model, derived outcome, verification, and
stored completion data. Run `aid show --help` because its modes are mutually
exclusive in some combinations.

Human task surfaces use verification tags only when verification has something
to say: `VFAIL` for a failed verification, `VTIMEOUT` for a timeout, `VINFRA`
for a verification infrastructure failure, and `VNORESULT` when a required
verification has no result. Running tasks and tasks without a verify command
have no tag. The tags appear in board rows, `aid show`, the TUI, and task detail.

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
```

Stopping preserves the worktree and attempts to preserve in-flight changes.
Inspect the artifact afterward. A retry creates linked history; use `tree` to
understand the chain.

`aid retry <task-id>` on a non-terminal task supersedes that task's own run: aid
stops the still-live worker first, then starts the new attempt in the same
worktree. If the worker cannot be stopped, the retry is refused. A retry still
refuses a worktree genuinely held by a different live task.

When the recorded linked worktree still exists, retry reuses it with the
original repository checkout as its anchor. Retry still refuses a target branch
that is genuinely checked out in the checkout that dispatched the task.

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
task was delivered. `aid stats` and `aid cost` show `unknown` when a task's
model has no known pricing. Unknown costs are omitted from totals rather than
recorded as `$0.00`.

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
6. Use `aid doctor` for evidence, not destructive repair.
