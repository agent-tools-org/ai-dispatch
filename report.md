Implemented and committed as `78c84812`.

- Removed all stash dropping and positional-selector logic.
- Removed branch-subject reconstruction; Git’s exact emitted subject is matched.
- Added detached-HEAD capture/restore coverage.
- Retained stash entries as durable Git-owned recovery records.
- Replaced the shifted-drop test with retention assertions.
- `54/54` merge tests, `aid build`, and clippy passed.
- Policy checks passed; worktree is clean.

Retention is intentionally owned by normal Git stash maintenance; `aid` never deletes these recovery entries. Concurrent merge locking and `merge --abort` remain out of scope.