# Merge-local stash custody audit

Scope: replacement commit `0aeecd59` (`fix: use durable stash entries for merge custody`). This is a read-only audit. The production path reviewed is `src/cmd/merge_stash.rs` plus its callers in `src/cmd/merge_git.rs` and `src/cmd/merge.rs`.

## 1. Can uncommitted user work be destroyed or become unfindable? — PASS

Evidence:

- Capture first runs `git status --porcelain` and fails closed on process or non-zero-status errors (`src/cmd/merge_stash.rs:99-107`). It does not silently proceed to a reset. `stash push --include-untracked` then creates a normal stash entry (`:121-144`).
- The stash is never dropped. Successful restoration uses `git stash apply --index <SHA>` (`:168-185`), which leaves the source stash in `git stash list`. A restore failure therefore leaves the original tracked and untracked snapshot available by SHA.
- If the process dies after `stash push` and before lookup, during merge, or before/inside restore, the normal stash entry remains. The worktree may be clean, conflicted, or partially restored, but the source snapshot remains visible and GC-reachable through the stash list.
- On a merge conflict, the code deliberately does not apply the tracked portion into the conflicted index. It restores only the untracked tree and reports the stash commit (`src/cmd/merge_git.rs:212-220`, `src/cmd/merge_stash.rs:55-90, 92-96`). A human can resolve the merge and apply the reported SHA.
- Before normal restoration, every untracked path from the stash is checked for an existing worktree path. A collision returns an error before `stash apply`; the complete stash remains available (`:47-53, 201-239`). If `git stash apply` itself fails partway, Git has still not dropped the stash, so the source remains the recovery authority.
- On capture lookup failure after a successful push, the returned error includes the generated message and tells the operator to search `git stash list` (`:39-44, 259-264`). That is weaker than a SHA but not an unfindable state.
- The existing regression test `cmd::merge::tests::git_merge_branch_fails_loudly_on_stash_restore_conflict` passed and verified that `untracked.txt` was present while the merge remained conflicted. The competing-stash and aggressive-GC tests also passed.

Concurrency qualification: there is no merge-wide repository lock. Two processes can race on status, stash push, merge, and `merge --abort`; one may fail or interfere with the other’s in-progress Git operation. The custody snapshots remain normal stash entries, but the implementation does not guarantee correct concurrent merge ownership. This is a correctness risk, not evidence that the stash source is destroyed or made unreachable.

## 2. Is identification actually exact? — FAIL

Evidence:

- The lookup command lists `%H` and `%gs`, but accepts the first entry where `subject.contains(message)` (`src/cmd/merge_stash.rs:146-166`). It does not require an exact generated message match. A hand-made stash whose reflog subject contains the generated message can be selected instead, especially if it is created between `stash push` and `stash list`.
- If two captures use the same message, stash-list order makes both lookups select the newest matching entry. Process A can apply process B’s SHA; process B can apply the same SHA again. A’s actual stash remains in the list, but it is not the SHA held by A, so restoration is wrong and the reported recovery handle can identify the wrong snapshot.
- The PID plus nanosecond timestamp makes ordinary independent processes unlikely to collide, but it is not an ownership protocol and there is no duplicate detection. The code’s lookup contract is therefore not exact under the collision cases requested by this audit.
- Once found, the returned full commit ID is used directly in `git stash apply --index <SHA>` (`:168-185`). No production call site in the reviewed tree uses `stash@{n}`, `stash pop`, or another positional stash selector. The only positional references found were historical audit prose, not executable code.

## 3. Did removal of the old machinery regress untracked/conflict behavior? — PASS

Evidence:

- `git stash push --include-untracked` is the single capture operation (`src/cmd/merge_stash.rs:121-144`), so untracked files are removed from the worktree with the tracked snapshot and remain in the durable stash rather than an external temporary directory.
- For a `-u` stash, Git produced a stash commit with three parents in a direct plumbing check. `stash^3` resolved to the untracked-files commit, and `ls-tree` of that parent contained the captured untracked file. This matches `stash_untracked_tree` (`:188-199`) and the later `^{tree}` extraction (`:67-77`).
- During a conflicted merge, `restore_untracked_after_failed_merge` enumerates that third-parent tree and runs `git restore --source <tree> --worktree` for those paths only (`:55-90`). It does not touch the conflicted index or branch-tracked files. The merge regression test verified the untracked file was actually visible in the worktree for human resolution.
- The old custom refs, temporary backup directory, expiry sweep, pre-reset snapshot check, and separate untracked-file machinery are absent from the current production tree. No old `refs/aid` or temporary-backup handle remains in the implementation.
- The full test run passed, including 77 merge-related tests. `aid build` and `aid build clippy` both completed with zero errors.

Known residual behavior: successful captures are intentionally never dropped, so every successful `aid merge` permanently grows the normal stash list and retains the captured objects. That preserves visibility and GC safety, but it creates unbounded operational accumulation and can expose sensitive historical work in the user’s stash list. This is not a data-loss regression, but deleting the old expiry sweep removed the only cleanup mechanism.

## What I missed / residual risks

- Exact identity needs an equality/ownership check, not `contains`, and a collision must fail closed rather than applying the first matching entry.
- The path has no repository-level merge lock. The stash SHA fixes the old top-of-stack selection bug, but it does not serialize two `aid merge` processes or protect a `check_merge` `merge --abort` from another merge process.
- There is no automatic cleanup or reconciliation for the retained successful stash entries. Operators need a documented, safe way to distinguish completed custody entries from unresolved recovery entries.
- I did not kill a live production process at each individual Git syscall boundary; crash conclusions are based on the fact that `stash apply` never drops the source and that `git stash push` publishes a normal stash entry. I also did not run a destructive fault-injection test against Git’s own mid-command failure behavior. Those cases remain unknown beyond the durable-source guarantee.

## Verification evidence

- `aid build`: `succeeded: 0 errors, 2 warnings; command: ... cargo check --bin aid`.
- `aid build clippy`: `succeeded: 0 errors, 8 warnings; command: cargo clippy`.
- `aid test --bin aid merge`: `passed: 77 passed, 0 failed, 0 ignored`.
- `aid test`: `passed: 2387 passed, 0 failed, 9 ignored; ... cargo test` (2396 tests selected/executed by the harness output).
- The `aid` wrapper reported it fell back to a temporary target because the requested shared `CARGO_TARGET_DIR` was considered unwritable; I did not override `CARGO_TARGET_DIR`.

## Overall verdict: FIX

The replacement is materially safer than the earlier positional-pop and temporary-file designs, and custody passes the requested crash, conflict, collision-preservation, and partial-restore cases. It is not ready to ship because the central “exact message” identity claim is false under colliding messages, and concurrent merge operations are not serialized. Fix identity matching/collision handling first; then decide and document the retained-stash cleanup policy.
