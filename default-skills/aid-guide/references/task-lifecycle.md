# Principal Acceptance and Artifact Custody

## State model

Execution and custody are separate:

```text
task execution: Pending -> Running -> Done/Failed/Stopped -> Merged
principal review: Unreviewed -> Accepted | Rejected
artifact custody: Preserved -> Durability proved -> Deleted
```

`Done` means the agent execution completed. `Merged` means code was integrated.
Neither means the principal accepted the result.

## Review

Before deciding:

```bash
aid show <task-id> --summary
aid show <task-id> --diff
aid show <task-id> --result
aid show <task-id> --events
```

Confirm the requested outcome, verification evidence, final branch and commit,
uncommitted files, submodule changes, and any audit findings.

## Accept

```bash
aid accept <task-id>
```

Acceptance is an explicit principal act. It records the decision, principal,
accepted head, branch, and artifact manifest. It does not immediately delete
anything.

If the artifact changes after acceptance, review it again and issue a new
acceptance record. Decisions are append-only; the latest decision governs.

## Reject

```bash
aid reject <task-id>
```

Rejection preserves the worktree, branch, objects, output, and task evidence.
Use `aid retry` for a corrective attempt when appropriate. A later acceptance
must be another explicit decision.

## Custody GC

```bash
aid gc --task <task-id>
```

GC is allowed only when:

1. the latest decision is `Accepted`;
2. the worktree still matches the accepted head and manifest;
3. the worktree has no uncommitted or untracked artifacts;
4. the accepted superproject commit exists in durable Git storage;
5. every recursive submodule commit exists outside worktree-private storage;
6. every required commit is reachable from a durable branch, remote, or tag.

On success AID stores a durability certificate, then removes only the accepted
task worktree. It retains the branch/ref required for durability.

## Why raw prune is forbidden

For a linked worktree, a submodule can store unique objects under:

```text
<main-repo>/.git/worktrees/<worktree>/modules/<submodule>
```

Deleting the worktree's Git metadata can destroy those objects even when the
superproject branch survives. Therefore these are unsafe:

```text
git worktree prune
git worktree remove <aid-task-worktree>
rm -rf <aid-task-worktree>
git branch -D <aid-task-branch>
```

Never recommend them for AID-managed task artifacts.

## Failure handling

If GC refuses:

- dirty artifact: inspect, commit intentionally, review, and accept again;
- head or manifest changed: review the new state and accept again;
- missing durable object/ref: publish or preserve the submodule commit in its
  persistent repository, then retry proof;
- rejected/unreviewed task: do not delete it;
- missing task ownership: stop and investigate rather than pruning metadata.

The correct outcome of an inconclusive proof is preservation.
