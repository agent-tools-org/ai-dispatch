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

## Automatic loop protection

The streaming watcher stops an agent only when repeated tool, file, build,
test, commit, format, or lint activity both exceeds the repetition threshold
and persists for at least two minutes. A fast burst of duplicate events is not
enough. Brief density dips preserve the persistence clock; ten consecutive
below-threshold observations or a different dominant pattern end the run.
For custom text agents, repeated identical lines classified as reasoning also count
because their streams provide no richer event type;
structured-agent reasoning narration alone never justifies an automatic loop kill. Inspect
`aid show <task-id> --events` when a task reports that it was stopped as stuck.

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

## Recovery rules

1. Inspect `aid show <task> --events` and the worktree before acting.
2. Preserve failed, stopped, hollow-output, and missing-delivery artifacts.
3. Do not repair stale Git metadata with `git worktree prune`.
4. Do not force-reset an AID branch to make reuse succeed.
5. Resolve ownership and review first; use acceptance and custody GC only at
   the end of the lifecycle.
6. Use `aid doctor` for evidence, not destructive repair.
