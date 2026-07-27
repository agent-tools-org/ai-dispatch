# Principal-Acceptance Worktree Lifecycle

## Status and Severity

Proposed structural correction for a confirmed P0 data-loss defect.

AID currently treats agent completion, verification, merge, and artifact
disposal as if they were one lifecycle transition. A completed task can trigger
automatic worktree removal before the principal has inspected or accepted the
result. In a linked worktree with an initialized submodule, removal can delete
the submodule object database under:

```text
<superproject>/.git/worktrees/<worktree>/modules/<submodule>
```

The superproject branch survives in the shared object database while the
submodule commit named by its gitlink can disappear. Issue #866 confirmed this
failure mode: the superproject commit survived, but the submodule commit
containing the central `rpc.rs` fanout work did not.

## Non-Negotiable Invariant

> AID must not delete, prune, detach, garbage-collect, or make unreachable any
> task artifact until the principal explicitly accepts that task and every
> repository object needed to reconstruct the accepted result is proven durable.

Agent output, exit code, test success, verification success, elapsed time,
terminal status, batch settlement, branch merge detection, and disk pressure
are never substitutes for principal acceptance.

## Current Structural Defect

The current lifecycle collapses distinct facts:

```text
agent exits
  -> TaskStatus::Done
  -> optional verification
  -> worktree cleanup
```

Deletion authority is distributed across unrelated components:

- normal completion calls `cleanup_completed_worktree`;
- failed-task post-processing calls `cleanup_failed_worktree`;
- `--auto-gc` removes worktrees and branches after merge heuristics;
- `aid merge` marks a task merged and immediately removes its worktree;
- group merge does the same for each task;
- `aid worktree prune/remove` deletes paths without task acceptance evidence;
- doctor invokes raw `git worktree prune`;
- worktree creation invokes `git worktree prune` to resolve stale metadata.

The low-level delete functions receive repository paths, not task identity or
acceptance authority. No single component can enforce the safety invariant.

The message `Commits preserved on <branch>` checks only that the superproject
branch has commits ahead of main. It does not inspect initialized submodules,
gitlinks, alternate object databases, remotes, bundles, or object reachability.

## Lifecycle Model

### Execution state

Keep process execution separate from delivery custody:

```text
Waiting -> Running -> Done | Failed | Stopped
```

`Done` means only that the agent submitted a deliverable. It is not acceptance
and never authorizes deletion.

### Acceptance state

Add an independent acceptance record rather than overloading `TaskStatus`:

```text
Unreviewed -> Accepted | Rejected
```

The stored record contains:

```text
task_id
decision
decided_at
principal_id
source
accepted_head_sha
accepted_branch
artifact_manifest_digest
```

The acceptance command is an explicit principal action:

```text
aid accept <task-id>
aid reject <task-id> --reason <text>
```

`aid merge` may offer `--accept` or call the same acceptance service after a
successful principal-initiated merge. Merely observing `TaskStatus::Merged` is
not acceptance. Historical merged tasks have no acceptance record and therefore
remain ineligible for deletion.

Hooks, agents, retries, background workers, batch orchestration, and lifecycle
post-processing cannot create acceptance records. Web and MCP callers must
carry an authenticated principal identity and invoke the same service.

## Durability Certificate

Acceptance is necessary but not sufficient for deletion. A task becomes
deletion-eligible only after a recursive durability check succeeds.

### Superproject

- The accepted commit resolves from a persistent object database.
- The accepted branch or another persistent ref reaches the commit.
- Any uncommitted or untracked task files make the check fail.
- The accepted commit matches the recorded task result.

### Submodules

For every gitlink recursively reachable from the accepted superproject commit:

1. identify the submodule repository and recorded gitlink SHA;
2. prove the SHA resolves outside the disposable worktree-specific gitdir;
3. prove a persistent ref, configured remote, or AID-owned recovery bundle
   retains the commit and its required history;
4. reject deletion if the submodule is dirty, untracked, unavailable, or
   ambiguous.

Checking `git cat-file -e <sha>` from inside the disposable worktree is not
proof: it may succeed only because the private object database still exists.
The check must run against the persistent repository location with disposable
object paths excluded.

### Certificate

Persist:

```text
task_id
checked_at
accepted_head_sha
repositories[]
  repository_identity
  required_sha
  durable_location
  proof_kind
manifest_digest
```

Any task mutation, branch movement, gitlink change, or new dirty state
invalidates the certificate.

## Single Deletion Gate

Introduce one feature module:

```text
src/artifact_custody/
  mod.rs
  acceptance.rs
  durability.rs
  deletion_gate.rs
  types.rs
```

All destructive worktree operations require:

```text
DeletionRequest {
    task_id,
    worktree_path,
    intent,
    requested_by_principal,
}
```

The gate returns a typed result:

```text
DeletionAuthorization::Authorized(certificate)
DeletionAuthorization::Denied(reason)
```

No caller may invoke `git worktree remove`, `git worktree prune`, filesystem
removal, branch deletion, or target cleanup for a task artifact directly.
Low-level removal functions become private to the custody module and require an
authorization value that cannot be constructed externally.

Denial is fail-closed. A database error, missing task mapping, missing
acceptance record, missing repository, unreadable submodule, or incomplete
certificate preserves everything.

## Complete Structural Migration

The correction ships as one coherent lifecycle change. There is no temporary
kill switch, compatibility fallback, legacy cleanup path, or partially safe
mode. The release is not complete until acceptance, durability proof, and every
destructive caller use the same custody boundary.

### Schema

Add append-only `task_acceptance` and `artifact_durability` tables. Do not infer
acceptance for existing tasks. Schema migration marks every historical task
unreviewed.

### Commands

- `aid accept`: record explicit acceptance after showing task identity, branch,
  final commit, dirty state, and submodule summary.
- `aid reject`: preserve artifacts and record the reason.
- `aid gc --task <id>`: require acceptance and a fresh durability certificate.
- `aid gc --eligible`: delete only tasks individually authorized by the gate.
- `aid merge`: merge only; optionally accept through an explicit flag and
  visible confirmation contract.

### Status presentation

Board and show distinguish:

```text
DONE / UNREVIEWED
DONE / ACCEPTED
DONE / REJECTED
MERGED / UNREVIEWED
```

No UI may display a task as safely cleanable based only on `Done` or `Merged`.

## E2E-First Acceptance Matrix

The emergency patch begins with failing tests:

| Flow | Expected custody result |
|---|---|
| Done task with ordinary commit | Worktree and gitdir remain |
| Failed task with partial edits | Worktree and gitdir remain |
| Stopped task | Worktree and gitdir remain |
| Done task with `--auto-gc` | Worktree and branch remain |
| Principal runs `aid merge` | Merge succeeds; worktree remains |
| `aid worktree prune` sees tracked unaccepted task | Refuses deletion |
| Doctor apply sees tracked unaccepted task | Refuses deletion |
| Batch/group settles | Every task worktree remains |

The structural phase adds:

| Flow | Expected custody result |
|---|---|
| Agent or hook attempts acceptance | Rejected as unauthorized |
| Historical Merged task without acceptance record | GC denied |
| Principal accepts clean ordinary repository task | Eligible after proof |
| Principal accepts dirty task | Acceptance may record; GC denied |
| Accepted task has private-only submodule commit | GC denied |
| Submodule commit exists only inside worktree gitdir | GC denied |
| Submodule commit pushed to persistent remote | Eligible after proof |
| Recursive nested submodule is not durable | Entire deletion denied |
| Certificate SHA differs from current branch/gitlink | Certificate invalid |
| Second GC request after successful deletion | Idempotent Missing result |

The #866 reproduction must be an E2E fixture:

1. create a superproject and local submodule remote;
2. create an AID linked worktree;
3. initialize the submodule inside that worktree;
4. commit a submodule-only change without pushing it;
5. commit the new gitlink in the superproject worktree;
6. mark the task Done;
7. exercise every cleanup entry point;
8. assert the worktree gitdir and submodule commit still exist;
9. accept the task and request GC;
10. assert GC is denied until the submodule commit is made durable.

## Recovery Audit

The emergency release also ships a read-only audit:

```text
aid audit artifact-custody
```

It reports:

- tasks whose recorded worktree no longer exists;
- surviving worktree metadata with private submodule object databases;
- superproject gitlinks whose SHAs cannot be resolved persistently;
- branches pointing to missing submodule commits;
- misleading historical cleanup events;
- candidate local objects, reflogs, packs, backups, or remotes that may recover
  lost commits.

The audit must not run prune, repack, expire, gc, fetch with pruning, or any
other mutation.

## Release Gates

- acceptance identity and authorization reviewed;
- every destructive entry point uses the single deletion gate;
- recursive submodule durability E2E green;
- historical tasks default to unreviewed;
- recovery audit tested against a disposable incident fixture;
- destructive operations emit an immutable record naming the acceptance and
  durability certificates used.
- ordinary completion, failure, stop, retry, merge, batch, doctor, and manual
  worktree flows pass custody E2E coverage.
- CLI help and messages no longer claim commits are preserved without a
  matching durability certificate.
- repository-wide search confirms no lifecycle path directly invokes
  `git worktree remove`, `git worktree prune`, branch deletion, or artifact
  removal outside the custody module.
- old cleanup functions and configuration switches are deleted in the same
  release; no legacy fallback remains.

## Required Implementation Order

1. Add failing custody E2E tests, including the complete #866 reproduction.
2. Add acceptance records and principal-only commands.
3. Add recursive durability certificates.
4. Centralize destructive operations behind the deletion gate.
5. Migrate every lifecycle, merge, doctor, and worktree caller.
6. Delete old cleanup functions, raw prune calls, and obsolete configuration.
7. Add and run the read-only recovery audit on existing AID projects.
8. Release only when every gate above passes as one complete change.
