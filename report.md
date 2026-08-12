Implemented and committed as `09318446`.

- Exact full stash identity matching; duplicate matches fail closed.
- Successful restores drop only the verified SHA’s stash entry.
- Shifted stash positions refuse deletion rather than dropping another entry.
- Failed/conflicted merges retain the stash for recovery.
- Added regressions for duplicate identity and shifted-list cleanup.
- 54/54 merge tests passed; `aid build` and clippy passed.
- File-size, headers, unwrap, and staging checks passed.
- Worktree is clean.

Concurrent merge locking and `merge --abort` handling remain pre-existing and out of scope.