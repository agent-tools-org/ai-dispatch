Implemented and committed as `6ce49ddc`.

- Replaced custom refs, temp backups, expiry sweep, and snapshot machinery with durable `git stash push --include-untracked`.
- Identifies the exact stash by a unique message and restores by commit SHA; entries remain visible in `git stash list`.
- Preserved fail-closed status handling and complete collision reporting.
- Updated conflict recovery to restore only untracked files from the stash’s third parent.
- `git stash create` cannot capture untracked files in the supported Git version, so `push -u` is required for one durable combined snapshot.

Validation: 52 merge tests passed, `aid build`, clippy, policy checks passed, and the worktree is clean.