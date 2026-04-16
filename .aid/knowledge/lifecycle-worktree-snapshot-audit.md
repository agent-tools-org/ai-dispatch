# Lifecycle Worktree Snapshot Audit

## Scope

This slice adds a single worktree snapshot boundary for dirty-state parsing and empty-diff classification.

Files reviewed:

- `src/worktree/snapshot.rs`
- `src/worktree.rs`
- `src/cmd/run_dirty.rs`
- `src/cmd/run_post.rs`
- `src/commit.rs`
- `src/commit/rescue.rs`

## Verdict

Approve with repository-level audit blocker noted.

The slice removes duplicate `git status --porcelain` parsing from lifecycle dirty assertion and rescue code. Runtime behavior is preserved while dirty entries, rescue filtering, and empty-diff checks now go through `capture_worktree_snapshot`.

## Findings

### No blocking findings in the slice

- `WorktreeSnapshot` owns raw status lines, parsed modified/untracked entries, and optional empty-diff classification.
- `run_dirty` now reads dirty status lines from the snapshot boundary.
- `run_post::worktree_is_empty_diff` now delegates to the snapshot boundary.
- `commit::has_uncommitted_changes` now uses snapshot status instead of a separate status command.
- `commit::rescue` reuses snapshot parsing and rescue filtering instead of maintaining a local parser.

## Follow-Up

### Other command-specific status checks still exist

This slice intentionally focuses on lifecycle and rescue paths. Other command-specific uses of `git status --porcelain` remain in merge/retry/worktree maintenance code and can be evaluated separately if they start diverging.

### Existing repo-wide audit blocker

`aic audit --diff /tmp/worktree-snapshot.diff` was blocked by the repository's existing `cargo clippy -- -D warnings` failure:

- `src/agent/gemini_support.rs` is loaded as a module multiple times through `src/agent/gemini.rs`.

## Evidence

- `cargo test worktree_snapshot -- --nocapture`
- `cargo test empty_diff_detection_respects_worktree_state -- --nocapture`
- `cargo test rescue_dirty_worktree -- --nocapture`
- `cargo test final_assertion -- --nocapture`
- `cargo test detects_dirty_git_repo -- --nocapture`
- `cargo test rescue_untracked_amends_commit -- --nocapture`
- `rg -n "status --porcelain|--untracked-files=all|parse_dirty_line|worktree_status_lines|git_diff_stat_output" src/cmd/run_dirty.rs src/cmd/run_post.rs src/commit.rs src/commit/rescue.rs src/worktree/snapshot.rs`
- `git diff --check`
- `aic audit --diff /tmp/worktree-snapshot.diff`:
  - blocked by existing `cargo clippy -- -D warnings` failure
