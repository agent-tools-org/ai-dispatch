Root cause: merge-local capture could treat failed status as clean, reset edits created after capture, and hide colliding untracked backups.

Fix committed in `ca9fa17b`:

- Fail closed when `git status` fails.
- Verify tracked worktree/index snapshots before reset.
- Report every untracked collision and backup path.
- Preserve recovery anchors until restoration completes.
- Expire anchors older than 30 days on the next capture.
- Split custody helpers to remain under 300 lines.

Validation:

- Pre-fix regressions reproduced.
- `aid test`: 53/53 merge tests passed.
- `aid build`: passed.
- `aid build clippy -- --all-targets`: passed with existing warnings.
- Worktree clean.

The group-merge cleanup and `git merge --abort` paths were pre-existing and untouched.