Root cause: `git stash pop` restored the current top stash, not the stash created for that merge. Untracked files were also not captured.

Reproduction succeeded: task-B’s newer rescue stash was restored after merging task-A, while the merge stash remained. The pre-fix regression failed with `task-b rescue` instead of `local change`.

Fix committed as `d26cf6a9`:

- Captures the exact stash commit ID and restores that stash only.
- Includes untracked files.
- Fails loudly with the stash ID if restoration conflicts.
- Added regression coverage for stale and untracked stashes.

Verification: 47 merge tests passed; `aid build` and clippy passed. Working tree is clean.