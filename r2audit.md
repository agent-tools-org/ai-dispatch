1. THE ONE THAT MATTERS — FAIL

Evidence:

- Capture uses `git stash create`, anchors the commit, copies untracked files, then runs `reset --hard` (`src/cmd/merge_stash.rs:32-66`).
- If the process dies after anchoring but before reset, the worktree still contains the user’s files and the anchor ref remains. The user receives no message because the process died.
- If it dies after reset, tracked changes exist only in `refs/aid/merge-local/<sha>` and untracked changes exist only under `/tmp/aid-merge-local-*`. The temp backup has no durable manifest, fsync protocol, startup recovery, or cleanup. A reboot may preserve it, but an OS temp sweep can remove it and lose the untracked files.
- `has_local_changes()` fails open: any `git status` execution error is treated as “clean” (`src/cmd/merge_stash.rs:119-126`).
- Concurrent aid merges have no whole-operation repository/worktree lock. Individual Git commands may serialize on the index lock, but capture, reset, merge, and restore are not atomic. One invocation can reset or restore while another is merging. Same tracked content also produces the same anchor ref, allowing one invocation to delete the ref while another still relies on it.
- On capture/reset errors, recovery reporting is incomplete. `format_capture_error()` receives no untracked backup handle (`src/cmd/merge_stash.rs:56-64`, `286-292`), even if cleanup failed and the temp directory remains.
- On a normal successful restore, tracked data is applied, the anchor is deleted, then untracked data is restored (`src/cmd/merge_stash.rs:68-81`). Restore failures do name the known handles, but this does not cover process death or temp-directory deletion.

The design protects tracked data better than the old positional-stash approach, but untracked data can still become lost or unfindable, and concurrent invocations can corrupt custody.

2. Stack-free claim — FAIL

Narrow stack claim: PASS.

The production path contains no `stash@{n}` access and no positional pop. It uses:

- `git stash create` (`src/cmd/merge_stash.rs:128-140`)
- `git stash apply --index <commit-id>` (`src/cmd/merge_stash.rs:262-271`)
- `git update-ref` for the anchor (`src/cmd/merge_stash.rs:142-152`, `274-283`)

The remaining `git stash push` occurrence is test/hook setup only.

End-to-end custody claim: FAIL.

- Different commit IDs produce different ref names, so distinct snapshots do not normally collide.
- Identical concurrent snapshots share one ref. `update-ref` is unconditional, and one successful restore can delete the ref needed by another invocation.
- Failed merges deliberately retain the anchor: `preserve_changes_after_failed_merge()` restores only untracked files and returns without calling `drop_tracked_anchor()` (`src/cmd/merge_git.rs:247-259`).
- Process death also leaves the ref.
- There is no cleanup command, startup sweep, expiry policy, or other cleanup path. The only deletion is the successful-restore call at `src/cmd/merge_stash.rs:75-76`.

Thus an incomplete merge can leave permanent `refs/aid/merge-local/*` refs. This is safer than deleting the only recovery handle, but it is still a leak unless manual cleanup is explicitly part of the design.

3. Conflicted-merge fix — FAIL

Partial results:

- Single-task conflicts preserve the merge’s own `Failed` result, append custody details, and do not replace it with a restore error (`src/cmd/merge_git.rs:212-217`, `247-259`).
- The single-task caller still invokes `task_lifecycle::restore_after_merge_failure()` (`src/cmd/merge.rs:112-120`).
- The ordinary tested conflict path restores untracked files into the worktree. Focused tests passed:
  - `aid test --bin aid git_merge_branch`: 7 passed
  - `aid test --bin aid stash_`: 3 passed

However, untracked files are not guaranteed to be present while resolving conflicts. `restore_untracked()` refuses to overwrite an existing destination (`src/cmd/merge_stash.rs:250-259`). If the conflicted merge creates a path with the same name as one of the user’s untracked files, restoration fails and the user’s file remains only in the temp directory. The existing test covers an unrelated `untracked.txt`, not this collision.

Also, the group merge path skips failed tasks without calling `restore_after_merge_failure()` (`src/cmd/merge.rs:311-317`); only the single-task path performs that lifecycle transition.

What I missed in the first pass:

- The custody protocol still has a capture-to-reset TOCTOU window. New tracked edits made after `stash create` but before `reset --hard` are destroyed.
- `git status` failure is treated as a clean worktree.
- Capture/reset failure reporting can omit an existing untracked backup directory.
- No crash/reboot recovery or stale-anchor cleanup exists.
- No concurrency test exists for two aid merges operating on the same repository.
- No test covers an untracked file whose destination is created during a conflicting merge.
- `check_merge()` ignores failure from `git merge --abort` (`src/cmd/merge_git.rs:238`), so a failed abort can leave merge state behind.

Overall verdict: BLOCK

The stack-position race is fixed, but data custody is still not safe across process death, temp cleanup, concurrent merges, or destination collisions during conflicted merges.