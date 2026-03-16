# Common Pitfalls

## Agent Behavior Issues
- Agents may `git add .` and commit untracked files (README, claude-prompt.md) — scope guard warns but doesn't prevent
- `<aid-system-context>` tags leak into agent commit messages — workspace injection content appears in commits
- opencode sometimes runs `cargo fmt` without being asked — handled via team rules ("Do NOT run cargo fmt")
- Agents in worktrees may carry stale Cargo.lock if created before a version bump

## Test Isolation
- Tests that change `env::current_dir()` or `env::set_var()` MUST use `thread_local!` guards, not global `Mutex`
- `AidHomeGuard` pattern: set per-thread AID_HOME to tempdir, restore on drop
- Parallel `cargo test` runs tests in threads, not processes — shared global state causes flaky tests

## Worktree Edge Cases
- macOS `/var` → `/private/var` canonicalization breaks path comparisons — always canonicalize both sides
- Branch already checked out in another worktree → `existing_worktree_path()` detects and reuses or prunes
- `git worktree list --porcelain` format is the reliable parser input (not the human-readable format)

## SQLite
- `busy_timeout=5000` prevents "database is locked" under parallel task access
- Schema migrations in `store/mod.rs` use `ALTER TABLE ... ADD COLUMN` with IF NOT EXISTS pattern
- Always add `DEFAULT` values for new columns to avoid migration issues

## RunArgs
- `RunArgs` has 40+ fields with `Default` impl — when adding fields, update `Default` and any test initializers
- `args.verify` is `Option<String>` not `bool` — it holds the verify command string
- Team/project defaults are fallbacks only — CLI flags always take priority
