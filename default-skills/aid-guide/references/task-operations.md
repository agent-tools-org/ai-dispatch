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

Use `watch` for a stream and `wait` for automation. Use `show` to inspect
context, diff, output, result, transcript, audit, event log, agent, model, and
stored completion data. Run `aid show --help` because its modes are mutually
exclusive in some combinations.

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
- Use `unstick` when progress has stopped and recovery is appropriate.

Do not send repeated polling messages; inspect events first.

## Automatic safeguards

AID enforces configured idle, hung-task, cost, and maximum-duration safeguards.

`--idle-timeout SECS` stops a task whose stream goes quiet. Foreground streaming
measures this on raw output lines, so an agent that keeps emitting unparseable
output (a spinner, for example) resets the timer even though it produces no
parsed activity; the PTY watcher is what catches that case.

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

## Merge

```bash
aid merge <task-id> --check
aid merge --group <group-id> --approve
```

Merge integrates delivered code and records `Merged`. It does not accept the
delivery on behalf of the principal and does not authorize worktree deletion.
Review can happen before or after integration depending on team policy, but
`aid accept` must remain an explicit principal decision.

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
