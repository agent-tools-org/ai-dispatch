# Lifecycle Postprocess Phase Audit

## Scope

This slice extracts task post-processing side effects from `post_run_lifecycle`.

Files reviewed:

- `src/cmd/run_lifecycle.rs`

## Verdict

Approve with repository-level audit blocker noted.

The slice preserves ordering while moving done-task and failed-task post-processing into named helpers. The coordinator now delegates memory success accounting, empty-diff detection, fast-fail cleanup, result-file persistence, failure hooks, quota tracking, and failed worktree cleanup to a dedicated phase.

## Findings

### No blocking findings in the slice

- `run_task_postprocess_phase` returns the same `Option<String>` quota-message shape as the previous inline code.
- Done-task processing still clears rate-limit state, records memory success, and checks empty worktree diff.
- Result files are still persisted before failed-task worktree cleanup.
- Failed-task processing still marks quota failures, runs `on_fail`, and removes only unshared non-read-only worktrees.

## Follow-Up

### Completion side effects remain in the coordinator

Notification, output autosave, hollow-output detection, post-DONE audit, summary persistence, completion hooks, webhooks, announcements, and retry routing remain in `post_run_lifecycle`. These should be extracted in later slices.

### Existing repo-wide audit blocker

`aic audit --diff /tmp/lifecycle-postprocess-phase.diff` was blocked by the repository's existing `cargo clippy -- -D warnings` failure:

- `src/agent/gemini_support.rs` is loaded as a module multiple times through `src/agent/gemini.rs`.

## Evidence

- `cargo test audit_ -- --nocapture`
- `cargo test delivery_assessment -- --nocapture`
- `cargo test final_assertion -- --nocapture`
- `git diff --check`
- `aic audit --diff /tmp/lifecycle-postprocess-phase.diff`:
  - blocked by existing `cargo clippy -- -D warnings` failure
