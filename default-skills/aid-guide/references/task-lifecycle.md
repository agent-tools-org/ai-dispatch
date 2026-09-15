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
`InfrastructureFailure`, or `Skipped`. `Unobserved` is reserved for an agent
exit with no surviving completion event —
not a verify result, an unknown process outcome. `TimedOut`,
`InfrastructureFailure`, and `Unobserved` are inconclusive rather than
evidence that the change is broken. A build environment that cannot run is one of those cases: when
cargo is denied write access to the configured target directory, aid retries
against a writable temporary target, and if that still fails the task is
recorded `InfrastructureFailure` (`Unverified`) rather than `Failed`
(`Broken`). `TaskOutcome` derives the judgment from lifecycle, verification, and
delivery assessment: only `Verified` and `Delivered` are success; `Unverified`
and `Broken` are not. A `Done` task with `delivery_assessment=hollow_output` or
`missing_final_delivery` is judged `Failed` — those assessments mean nothing
was observed on any delivery channel (output, transcript, log, and worktree
changes), not merely quiet stdout. `empty_diff` alone does not demote success:
a report-only audit or commit-cleaned worktree can still be a real delivery.

## Artifact backup

When a project or `aid run --backup` configures a backup target, a task that
ends in `Done` or `Failed` (matching `on = ["complete", "fail"]`) has its
export, diff, and raw log bundled and uploaded. A backup is attempted once,
after the post-run lifecycle of a task that ran: after verification, the verify
gate, and result-file persistence, so the bundle carries the final status. Tasks
ended by `aid stop`, by the background reaper (dead worker, idle, timeout,
pending or waiting timeout), or by a failure before the agent started are not
backed up. The resulting URL is stored on the task (`aid show` prints `Backup:
<url>`; `--json` carries `backup_url`) and a milestone event records it. Backup
is observational only: a failed attempt, including a `[backup]` config error,
counts as the task's one attempt and is never retried; it adds a milestone event
beginning `Backup failed:` plus a stderr line while `latest_error` keeps the
agent's own error; and neither success nor failure alters `TaskStatus`,
`VerifyStatus`, `TaskOutcome`, or the exit code. See the configuration reference
for the `[backup]` keys.

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
2. the manifest still matches acceptance, and a live worktree matches the accepted head;
3. a live worktree has no uncommitted or untracked artifacts;
4. the accepted superproject commit exists in durable Git storage;
5. every recursive submodule commit exists outside worktree-private storage;
6. every required commit is reachable from a durable branch, remote, or tag.

On success AID stores a durability certificate, then removes only the accepted
task worktree. It retains the branch/ref required for durability.

If the worktree is absent on disk and unregistered in a successful
`git worktree list --porcelain`, GC reports that it was already collected.
Only the live HEAD and cleanliness checks are skipped. Object, ref, and manifest
proof still run using the repository's common Git directory reached from
`task.repo_path`; GC compares the manifest and records durability before reclaiming
fallback target directories. GC never runs repository-wide `git worktree prune`,
which could destroy another task's worktree-private objects. A failed listing
refuses collection.

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
