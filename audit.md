## Findings

**1. Time-of-Check to Time-of-Use (TOCTOU) race conditions in stash capture and restore**
- **Severity:** Critical
- **Files:** `src/cmd/merge_stash.rs`
- **Evidence:** 
  - *Capture race:* `stash_local_changes` executes `git stash push` and then `git rev-parse stash@{0}` as separate shell commands. If a concurrent process pushes a stash between these calls, `rev-parse` returns the hash of the concurrent stash.
  - *Restore race:* `restore_stash` resolves the hash to a positional index via `stash_selector` (e.g., `stash@{1}`), then executes `git stash pop stash@{1}`. If any stash is pushed or dropped between these commands, the indices shift.
  - *Impact:* The tool will pop the wrong stash, injecting incorrect data into the working tree, and abandon the user's actual stash silently.

**2. Overwritten merge conflict status breaks task lifecycle cleanup**
- **Severity:** High
- **Files:** `src/cmd/merge_git.rs`, `src/cmd/merge.rs`
- **Evidence:** 
  - In `git_merge_branch`, if the merge conflicts, it evaluates to `MergeResult::Failed`. It then unconditionally attempts `restore_stash`.
  - `git stash pop` fails because the index contains unmerged paths.
  - `restore_stash` returns an `Err`, causing `git_merge_branch` to overwrite the merge result and return `MergeResult::StashRestoreFailed`.
  - The caller (`merge_single_with_output`) matches on `StashRestoreFailed` and exits, skipping the `crate::task_lifecycle::restore_after_merge_failure` cleanup designed for merge conflicts.

**3. Untracked files unexpectedly trapped in stash on merge conflict**
- **Severity:** Medium
- **Files:** `src/cmd/merge_stash.rs`
- **Evidence:** 
  - `--include-untracked` successfully sweeps up untracked, un-ignored files (e.g., `report.md`, new source files).
  - When a merge conflict occurs, the stash deliberately fails to pop (preventing index overwrites). However, the untracked files are therefore NOT restored. The caller expects their working state to remain intact for conflict resolution, but instead, their new untracked files have vanished into the stash.

## Analysis by Question

### Question 1: Stash Restoration Sequences
**FAIL.** 
- **Another stash created between push and rev-parse:** FAIL. `rev-parse stash@{0}` captures the concurrent stash's hash. The tool restores and drops the wrong stash, abandoning the correct one.
- **Two aid merges running concurrently in the same repo:** FAIL. Same TOCTOU race. Both merges may read the same `stash@{0}`, leading to one successfully popping the other's stash and the original being abandoned.
- **A stash the user made by hand:** FAIL (if made in the exact window between push and rev-parse). PASS otherwise.
- **The same content stashed twice producing identical hashes:** PASS. `stash_selector` picks the first match. Since the contents are identical, restoring either produces the correct working tree.
- **A merge that fails before restore:** FAIL. The merge conflict fails the merge step, but `git_merge_branch` still attempts restore. The pop fails, returning an error that overwrites the original merge conflict status.

### Question 2: Loss of Uncommitted Work
**FAIL.**
The user's work is effectively lost from the working tree in two scenarios:
1. **Race Condition Path:** If the TOCTOU races hit, the wrong stash is popped and the tool prints `[aid] Restored local changes`. The user's actual work remains in the stash list under a generic `aid: auto-stash` message, but the working tree contains garbage data.
2. **Merge Conflict Path:** When the merge conflicts, `aid` is supposed to leave the merge for a human. However, because the restore fails, the return value is overwritten to `StashRestoreFailed`. The task lifecycle cleanup is bypassed. The stash is theoretically discoverable because the hash is printed in the error, but the working state is broken.

### Question 3: --include-untracked Surprises
**FAIL.**
- **Sweeps up:** New un-added source files, agent result files (like `report.md`), and untracked editor droppings. It does *not* sweep up build outputs in `CARGO_TARGET_DIR` because they are ignored or external.
- **Surprises:** If a merge conflict occurs, the stash is deliberately left unrestored by git. Because `--include-untracked` was used, all untracked files are wiped from the worktree and hidden inside the stash. This severely disrupts a user trying to resolve a conflict who relies on those untracked files (e.g., `report.md` instructions).

## Construction Sites of Changed Fields
The `MergeResult` and `MergeCheckResult` enums had the `StashRestoreFailed(String)` variant added.
- `src/cmd/merge_git.rs`: Constructed during `stash_local_changes` failure and `restore_stash` failure in both `git_merge_branch` and `check_merge`. (Updated correctly).
- `src/cmd/merge.rs`: Matched in `merge_single_with_output`, `merge_group_with_output`, `check_single`, `check_group`, and `print_check_result`. (All updated, though `merge_single_with_output`'s handling incorrectly skips lifecycle cleanup on conflict).
- `src/cmd/merge/tests.rs`: Matched in `check_merge_detects_conflict` and `git_merge_branch_fails_loudly_on_stash_restore_conflict`. (Updated correctly).

## Open Questions
- **What did I miss?** The second TOCTOU race vulnerability. Passing `stash@{N}` from `stash list` output directly into `git stash pop` is just as race-prone as `rev-parse stash@{0}`.
- **Could not check:** I could not check how `restore_after_merge_failure` exactly manipulates the state store because those files were not in the diff, leaving the exact severity of the bypassed cleanup unknown.

## Verdict
**BLOCK**

=== AID TASK t-446448ab DONE (exit 0) ===
