# Merge-local stash custody audit

## 1. Can uncommitted user work be destroyed or become unfindable? — PASS

Capture fails closed when `git status` fails. `git stash push --include-untracked` creates a durable normal stash entry, and restoration uses `git stash apply --index <SHA>` without dropping it.

Therefore:

- Process death after capture leaves the stash visible and GC-reachable.
- Merge conflicts leave tracked changes in the stash and restore untracked files for human resolution.
- Partial restore failures retain the original stash.
- Collision checks fail before applying and report the stash commit.
- The existing conflict, competing-stash, and aggressive-GC tests passed.

Qualification: there is no repository-wide merge lock. Concurrent processes can interfere with merge/index operations, although the custody snapshots remain durable.

## 2. Is identification actually exact? — FAIL

`find_stash` accepts the first entry where:

```rust
subject.contains(message)
```

This is substring matching, not exact matching. A hand-made stash with a colliding subject can be selected, especially if created between capture and lookup.

If two captures use the same message, both select the newest matching stash. Process A may apply process B’s SHA, and B may apply it again. A’s actual stash remains visible, but restoration is incorrect and the reported handle may identify the wrong snapshot.

After lookup, the full commit SHA is used directly with `git stash apply --index`. No production path still uses `stash@{n}`, `stash pop`, or another positional selector.

## 3. Did deleting the old machinery regress untracked/conflict behavior? — PASS

`git stash push --include-untracked` captures tracked and untracked changes together in the durable stash list.

A direct Git plumbing check confirmed:

- The stash has three parents.
- `stash^3` is the untracked-files commit.
- Its tree contains the captured untracked file.
- `stash^3^{tree}` is correctly usable as the restore source.

During a conflicted merge, only the third-parent tree is restored with `git restore --worktree`; the conflicted index and tracked merge files are untouched. The regression test verified the untracked file was present while a human resolves the conflict.

The removed custom refs, temporary backup directory, expiry sweep, snapshot check, and separate untracked-file machinery are absent.

Residual issue: successful captures are never dropped, so the normal stash list grows without bound and retains historical work.

## What I missed

- Exact identity requires equality/ownership validation, not `contains`; collisions must fail closed.
- No repository-level lock serializes concurrent `aid merge` processes or protects `merge --abort` from another merge.
- There is no cleanup or reconciliation policy for permanently retained successful stash entries.
- I did not perform syscall-level crash injection or destructive fault injection against Git itself.

## Verification

- `aid build`: 0 errors, 2 warnings.
- `aid build clippy`: 0 errors, 8 warnings.
- `aid test --bin aid merge`: 77 passed, 0 failed.
- `aid test`: 2387 passed, 0 failed, 9 ignored.

## Overall verdict: FIX

Custody is materially safer and passes the requested crash, conflict, and partial-restore scenarios. The central exact-identity claim is false under message collisions, and concurrent merge operations are not serialized.