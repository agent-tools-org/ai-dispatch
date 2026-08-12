# Lifecycle Worktree Snapshot Audit Pass 3: Local-Change Custody

## 1. THE ONE THAT MATTERS
**FAIL**

There is a direct data-loss path in `backup_untracked` where a user's uncommitted untracked work is permanently destroyed without a recovery handle.

**Evidence:**
In `src/cmd/merge_stash_files.rs`, if `fs::remove_file(&source)` fails when clearing the worktree, the error handler attempts to rollback by restoring the already-removed files:
```rust
if let Err(error) = fs::remove_file(&source) {
    for restored in &removed {
        let _ = copy_entry(&root.join(restored), &Path::new(repo_dir).join(restored));
    }
    let _ = fs::remove_dir_all(&root);
    return Err(format!("failed to clear untracked file..."));
}
```
If `copy_entry` fails for any file (e.g., due to permissions, file locks, or disk space), the `let _ =` silently ignores the failure. The file is not returned to the worktree. Then, `fs::remove_dir_all(&root)` executes unconditionally, deleting the backup directory. The function returns an error without returning the `LocalChanges` object, so the caller never prints a recovery handle. The file is permanently destroyed in both places.

## 2. Does the pre-reset verification actually close the window?
**FAIL (It only narrows it)**

The pre-reset verification drastically narrows the window but does not close it.

**Evidence:**
In `capture_local_changes`, the code runs `verify_snapshot(...)` followed by `clear_worktree(...)`.
`verify_snapshot` spawns a `Command` for `git diff --quiet <snapshot>` to check the worktree, and another for the index. If they exit with 0, it proceeds to spawn `git reset --hard HEAD` in `clear_worktree`.
Because these are separate shell processes, there is a distinct race condition. If an IDE auto-save or background process writes an edit to a tracked file *after* `git diff --quiet` returns 0 but *before* `git reset --hard HEAD` executes, that edit is not in the captured stash but will be permanently destroyed by the reset. A narrowed window is acceptable, but it is unequivocally not closed.

## 3. Is the 30-day anchor expiry safe?
**FAIL**

The 30-day anchor expiry is unsafe, silently deletes unrecovered data, and uses a dangerous trigger.

**Evidence:**
In `sweep_stale_anchors`, it iterates over `refs/aid/merge-local` and parses `%(creatordate:unix)`. For commits, this is the committer date (when the stash was created). If this timestamp is older than 30 days, the ref is deleted via `git update-ref -d`.
This sweep runs on the *next* `capture_local_changes`, which triggers on every new `aid merge`. If a user had a merge conflict 31 days ago, left the anchor unrecovered, and runs an unrelated `aid merge` today, their 31-day-old anchor is silently deleted without warning. The unrecovered commit becomes orphaned and will be permanently destroyed by the next `git gc`. An unrelated merge is not an appropriate trigger to silently expire user backups.

## What did I miss?
1. **Untracked backups are placed in the OS temp directory.** `unique_backup_root` uses `std::env::temp_dir()`. If a merge fails and the user leaves the untracked files in the recovery handle for later, the operating system's temp file sweeper (or a reboot) may permanently delete their untracked work before they return to resolve the merge. A repo-local `.git/` directory would be safer.
2. **Partial untracked restore duplication.** In `restore_untracked`, if `copy_entry` fails midway, it returns an error and skips `fs::remove_dir_all`. While this safely preserves the backup, the files that *were* successfully copied back are now in both the worktree and the backup, which could confuse a user manually recovering from the printed handle.
3. **Stale anchor sweep short-circuiting.** If `drop_stale_anchor` fails for one ref, the `?` operator aborts the loop, leaving older anchors untouched.

## VERDICT
**BLOCK**

=== AID TASK t-8a207436 DONE (exit 0) ===
