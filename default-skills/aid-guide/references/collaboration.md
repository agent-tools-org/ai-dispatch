# Batch and Collaboration Workflows

## Batch task graph

Generate a maintained example:

```bash
aid batch init
aid batch tasks.toml --analyze
aid batch tasks.toml --parallel --max-concurrent 3 --wait
```

Use `[defaults]` for shared agent, directory, verification, team, fallback,
context, skill, and budget settings. Use `[[task]]` entries with unique names,
prompts, worktrees, and `depends_on` edges. Prefer dependencies over parallel
tasks editing overlapping files.

Useful controls:

- `--analyze` reports likely path conflicts.
- `--parallel` enables concurrent ready tasks.
- `--max-concurrent` caps concurrency.
- `--wait` blocks for the group result.
- `--dry-run` validates without dispatch.
- `--yes` or `--no-prompt` supports non-interactive automation.
- `--var key=value` supplies batch interpolation variables.

Retry failed batch members with `aid batch retry --help`; preserve the original
workgroup so history remains connected.

## Workgroups

```bash
aid group create release --context docs/plan.md
aid group list
aid group show <group-id>
aid group summary <group-id>
aid group broadcast <group-id> "Integration starts now"
aid group cancel <group-id>
```

Groups organize tasks and shared context. Group deletion is administrative; do
not treat it as artifact acceptance or cleanup.

## Findings

```bash
aid group finding add --group <group-id> \
  --title "Race in scheduler" \
  --severity high \
  "Workers can claim the same task"
aid group finding list --group <group-id>
```

Use findings for reviewable evidence with source task, file, line, category,
confidence, verdict, score, and note metadata. Use `aid group finding --help`
for exact subcommands and flags.

## Teams

```bash
aid team list
aid team show dev
aid team create dev
```

Teams provide reusable agent and instruction groupings. Select them with
`aid run --team` or batch defaults.

## Memory

```bash
aid memory add convention "Use explicit Result types" --tier critical
aid memory list
aid memory search "Result"
aid memory update <memory-id> "Updated convention"
aid memory history <memory-id>
```

Use memory for durable project knowledge that should influence future tasks.
Keep entries specific, non-obvious, and attributable. Use `forget` only when
the fact should no longer be available; use `update` when preserving history is
important.

## Knowledge graph

```bash
aid kg add --help
aid kg query --help
aid kg timeline --help
aid kg invalidate --help
aid kg search --help
aid kg stats
```

Use the knowledge graph for temporal entity relationships. Invalidate facts
that cease to be true instead of silently overwriting history.

## Batch review and acceptance

Review each task independently, even when the group succeeds:

```bash
aid group summary <group-id>
aid show <task-id> --summary
aid show <task-id> --diff
aid accept <task-id>
aid gc --task <task-id>
```

A group summary, group merge, or successful dependency graph never constitutes
principal acceptance for every member.
