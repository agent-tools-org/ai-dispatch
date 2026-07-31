# Dispatch and Execution

## Choose the entry point

- Use `aid run` for one accountable task with a stored lifecycle.
- Use `aid batch` for multiple dependent or parallel tasks.
- Use `aid ask` for quick research with file context.
- Use `aid query` for a direct model query that does not need a task worktree.
- Use `aid benchmark` to compare agents on the same prompt.
- Use `aid experiment` for repeated metric-driven improvement.
- Use `aid build` for compact Rust build/test diagnostics.

## Run one task

```bash
aid run codex "Implement request validation" \
  --dir . \
  --worktree feat/request-validation \
  --verify \
  --retry 1 \
  --bg
```

Important controls:

- `--dir` sets the task working directory.
- `--repo` or `--repo-root` supplies the repository anchor.
- `--worktree` creates or reuses an isolated task branch.
- `--verify [COMMAND]` verifies completion; without a value it uses project
  configuration or supported defaults.
- `--retry N` permits new attempts after failure.
- `--bg` returns the task ID immediately.
- `--read-only` forbids writing intent.
- `--sandbox` requests sandboxed execution.
- `--timeout SECS` is a hard wall-clock cap in seconds.
- `--idle-timeout SECS` stops a task without parsed activity.
- `--audit` runs the configured cross-audit.
- `--result-file` requires a durable result artifact.
- `--output` selects a task output path.

Run `aid run --help` for iteration, evaluation, judging, peer review, best-of,
model, budget, context, scope, checklist, skill, template, hook, container, and
cascade options.

## Context and instructions

Use `--context <path>...` for source material, not extra positional arguments.
Use `--context-from <task>...` to inject prior task output. Use `--scope` to
state intended files. Use `--checklist` or `--checklist-file` for explicit
acceptance criteria.

Prefer a project verify command for consistent results:

```toml
[project]
id = "example"
verify = "cargo test --bin app"
```

## Result delivery

When `--result-file` is set (audit and review prompts set it automatically) and
the agent never writes that file, AID salvages the captured agent output into
the task's `result.md` so evidence is not lost. If that output is pre-tool
narration rather than a report, AID records a `missing_final_delivery`
assessment and an error event. `aid show` then prints the missing-result banner
instead of presenting the tool log as findings. Treat that banner as "no audit
happened" and re-dispatch.

## Worktree safety

```bash
aid worktree create feat/change
aid worktree list
```

AID task worktrees are custody containers, not disposable scratch directories.
Completion, failure, stop, retry, and merge preserve them. Do not use raw
`git worktree prune` or direct `git worktree remove` on them.

If AID reports missing worktree registration or conflicting metadata, stop and
identify the owning task. Automatic pruning is intentionally forbidden because
linked-worktree Git metadata may contain unique submodule objects.

## Build

```bash
aid build check
aid build test --package my-crate
```

Use `aid build --help` for supported Cargo command and filter options. It emits
compact, deduplicated diagnostics and integrates progress into task events.

## Retry and fallback

```bash
aid retry <task-id> --feedback "Address the failed invariant"
aid run codex "Task" --cascade opencode,cursor
```

A retry is a new attempt linked to its parent. It does not erase or rewrite the
failed attempt. Inspect the tree with `aid tree <task-id>`.

## Verify before review

```bash
aid wait <task-id>
aid show <task-id> --summary
aid show <task-id> --diff
aid show <task-id> --result
```

Agent success and verification are evidence for review, not acceptance.
