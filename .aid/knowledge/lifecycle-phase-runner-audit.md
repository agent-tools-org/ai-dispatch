# Lifecycle Phase Runner Audit

## Scope

This slice introduces a small phase decision model and extracts the opening part of `post_run_lifecycle` into named phases.

Files reviewed:

- `src/cmd/run_lifecycle.rs`

## Verdict

Approve with repository-level audit blocker noted.

The slice is a low-risk structural refactor. It preserves lifecycle ordering while making teardown, worktree settlement, verification/scope checks, and checklist scanning explicit phases.

## Findings

### No blocking findings in the slice

- `LifecyclePhaseDecision` converts dirty-worktree outcomes into coordinator-level decisions.
- `run_teardown_phase` owns sandbox cleanup and worktree lock clearing.
- `run_escape_checks_phase` owns scope escape and worktree escape checks.
- `run_worktree_settlement_phase` owns post-agent dirty worktree handling and early retry/stop decisions.
- `run_verify_scope_phase` keeps verify and scope violation checks together.
- `run_checklist_phase` and `record_checklist_result` isolate checklist output scanning and event recording.

## Follow-Up

### More lifecycle phases remain inside the coordinator

This slice only extracts the front half of `post_run_lifecycle`. Follow-up slices should continue extracting task post-processing, completion side effects, and retry/cascade routing.

### Existing repo-wide audit blocker

`aic audit --diff /tmp/lifecycle-phase-runner.diff` was blocked by the repository's existing `cargo clippy -- -D warnings` failure:

- `src/agent/gemini_support.rs` is loaded as a module multiple times through `src/agent/gemini.rs`.

## Evidence

- `cargo test final_assertion -- --nocapture`
- `cargo test checklist -- --nocapture`
- `cargo test audit_ -- --nocapture`
- `git diff --check`
- `aic audit --diff /tmp/lifecycle-phase-runner.diff`:
  - blocked by existing `cargo clippy -- -D warnings` failure
