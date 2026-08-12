# Principal Acceptance and Artifact Custody

## State model

Execution and custody are separate:

```text
task execution: Pending -> Running -> Done/Failed/Stopped -> Merged
principal review: Unreviewed -> Accepted | Rejected
artifact custody: Preserved -> Durability proved -> Deleted
```

`TaskStatus` describes lifecycle and integration. `Done` means the agent process
exited successfully **and** its CLI result
envelope did not report a terminal error. Streaming agents that exit 0 while
emitting a real failure envelope (for example Cursor/Claude
`{"type":"result","is_error":true}`, OpenCode/Gemini `{"type":"error",...}`,
or Qwen `[API Error: ...]`) are recorded as `Failed`. Ambiguous envelopes stay
`Done` rather than risk a false failure.

`Merged` means code was integrated. Neither `Done` nor `Merged` means that the
task succeeded or that the principal accepted the result.

Verification is a separate axis. A configured verify command starts with
`VerifyStatus::Pending` and ends as `Passed`, `Failed`, `TimedOut`,
`InfrastructureFailure`, or `Skipped`. `TimedOut` and
`InfrastructureFailure` are inconclusive rather than evidence that the change
is broken. `TaskOutcome` derives the judgment from lifecycle, verification, and
delivery assessment: only `Verified` and `Delivered` are success; `Unverified`
and `Broken` are not. A `Done` task with `delivery_assessment=hollow_output` or
`missing_final_delivery` is judged `Failed` — those assessments mean nothing
was observed on any delivery channel (output, transcript, log, and worktree
changes), not merely quiet stdout. `empty_diff` alone does not demote success:
a report-only audit or commit-cleaned worktree can still be a real delivery.

## Review

Before deciding:

```bash
aid show <task-id> --summary
aid show <task-id> --diff
aid show <task-id> --diff --branch
aid show <task-id> --result
aid show <task-id> --events
```

Confirm the requested outcome, derived task outcome, verification evidence, final branch and commit,
uncommitted files, submodule changes, and any audit findings.

`--diff` is scoped to the task's own baseline (`start_sha..HEAD`). A task dispatched
into a worktree that already carries commits — a retry, or a follow-up on the same
branch — gets a baseline above them, so that scope can be a truthful but tiny sliver
while the branch holds the delivered work. The diff stat says so when it happens;
`--diff --branch` widens the view to every commit since the branch left the default
branch. Read it before concluding a task produced nothing.

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
