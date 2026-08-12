[MILESTONE] Read the diff and understand the code.
[MILESTONE] Answered Q1: TOCTOU race condition in `drop_stash_if_exact`.
[MILESTONE] Answered Q2: Detached HEAD mismatch and identity uniqueness.
[MILESTONE] Answered Q3: Retain-on-failure path intact.
[MILESTONE] Formulate final report.

### 1. Can the drop ever remove an entry that is not ours?
**FAIL**

**Evidence & Trace:**
The verification and the drop are two separate `git` invocations separated by a race window.
1. `find_stash_selector` maps `stash_ref` (the SHA) to a dynamic stash selector (e.g., `stash@{0}`).
2. `verify_stash_selector` runs `git rev-parse --verify stash@{0}` and strictly checks that its returned SHA equals our `stash_ref`.
3. The custom `before_drop()` hook executes.
4. `verify_stash_selector` runs again and verifies `stash@{0}` still matches.
5. A totally separate invocation executes: `git stash drop --quiet stash@{0}`.

Because `git stash drop` relies on the positional selector (`stash@{0}`) rather than the commit SHA, this creates a Time-Of-Check to Time-Of-Use (TOCTOU) vulnerability. If any concurrent git operation (like another user process or background IDE task) pushes a new stash immediately after step 4 but before step 5, the stash list shifts. `stash@{0}` will now point to the newly pushed (unrelated) stash, and `git stash drop stash@{0}` will silently destroy it. `git stash drop` does not accept a direct SHA, meaning this race condition is fundamental to passing positional references between non-atomic commands.

### 2. Does fail-closed identity ever refuse OUR OWN entry?
**FAIL**

**Evidence & Trace:**
Yes, the identity matching permanently refuses to capture (and thus restore) our own entry when the user is in a detached HEAD state. 
- In `stash_subject()`, `git branch --show-current` is called. If the user is on a detached HEAD, this returns an empty string, and the expected subject is built as `WIP on (no branch): <message>`.
- However, modern Git's `git stash push` creates the stash message with `On (no branch): <message>`. 
- Because the diff introduces exact matching (`subject == expected_subject`), `WIP on (no branch)` will never equal `On (no branch)`. 
- The capture phase fails immediately after pushing the stash, permanently leaving the user's work locked in the stash without ever restoring it to the working tree. This is not a safe failure.

**Identity Uniqueness:**
An identity is uniquely generated using `std::process::id()` combined with a nanosecond-precision timestamp (`SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()`). Two merges by the same user in the same second, or a retried merge, will inherently be executed in a different process (different PID) and/or at a different nanosecond. Therefore, they will never produce identical identities and this specifically will not cause a permanent refusal to restore.

### 3. Is the retain-on-failure path still intact after adding the drop?
**PASS**

**Evidence & Trace:**
The failure paths correctly bypass the destructive drop.
In `restore_local_changes_inner`:
```rust
ensure_stash_untracked_paths_free(repo_dir, &changes.stash_ref)?;
apply_stash(repo_dir, &changes.stash_ref)?;
drop_stash_if_exact(repo_dir, &changes.stash_ref, before_drop)
```
If `apply_stash` encounters a merge conflict or any other error, it propagates the `Err` via the `?` operator. Execution halts immediately, meaning `drop_stash_if_exact` is never reached. Furthermore, no error-handling block in the diff introduces a `stash drop` fallback, ensuring that a conflicted or failed merge successfully retains its stash in the list.

### Open: What did I miss? / Construction Sites
- **Construction Sites**: The `LocalChanges { stash_ref }` struct has no changed fields, but its construction site in `capture_local_changes` was modified to accept new closure arguments (`before_identify`, `after_capture`). All call sites (`stash_local_changes`, `stash_local_changes_with_hook`, `stash_local_changes_with_identity_hook`) were updated to correctly pass the required closures.
- **Missing Edge Case**: If a repository has an extremely long branch name, we confirmed Git does not truncate the `stash_ref` subject (tested with 80+ character branch names and `%gs`), so exact matching remains reliable in standard branch states.

### VERDICT
**BLOCK**

=== AID TASK t-e3af1648 DONE (exit 0) ===
