# Lifecycle Refactor Roadmap

## Goal

Stabilize the `aid run` completion path by replacing the current monolithic post-run flow with small modules that own one decision each. The program targets three chronic problem areas:

1. Worktree settlement and dirty-state recovery
2. Delivery assessment versus verification semantics
3. Post-completion side effects such as audit, hooks, persistence, and cleanup

## Phase Plan

### Phase 1: Module Wiring and Duplicate Removal

- Wire the already-extracted `run_dirty` and `run_post` helpers into the live `run` path
- Remove duplicated implementations from `run_lifecycle.rs`
- Preserve behavior and existing storage schema
- Exit criteria:
  - No behavior change in dirty-worktree cleanup
  - No behavior change in post-DONE audit, quota rescue, or output auto-save
  - Targeted tests pass

### Phase 2: Delivery Assessment Model

- Introduce a delivery assessment concept separate from `VerifyStatus`
- Move `EmptyDiff` and `HollowOutput` semantics out of verification
- Update `aid show`, summary rendering, and store updates to use the new model
- Exit criteria:
  - UI no longer infers delivery quality from verify state
  - Terminal-state rendering uses explicit delivery assessment rules

### Phase 3: Worktree Snapshot Boundary

- Add one shared worktree snapshot module that classifies:
  - dirty files
  - artifact paths
  - empty diff state
  - rescuable versus ignored paths
- Make rescue, final assertion, and show rendering read the same snapshot facts
- Exit criteria:
  - One canonical parser for git worktree facts
  - No duplicate `git status --porcelain` interpretation paths

### Phase 4: Lifecycle Phase Runner

- Split `post_run_lifecycle` into explicit phases:
  - teardown
  - worktree settlement
  - verify and scope
  - delivery assessment
  - persistence and cleanup
  - side effects
- Convert branchy control flow into a small phase result model
- Exit criteria:
  - `run_lifecycle.rs` becomes a thin coordinator
  - Each phase is testable in isolation

### Phase 5: End-to-End Hardening

- Add scenario-driven tests for historically unstable paths:
  - non-git dir
  - failed task with result file
  - shared worktree cleanup
  - stdin `/dev/null`
  - no-HEAD repo rescue
  - hollow output with no diff
- Exit criteria:
  - critical lifecycle regressions covered by integration-style tests
  - follow-up feature work must pass the lifecycle matrix

## Slice Policy

- One logical slice per commit
- Each slice must reduce coupling or duplicate logic, not just move code
- No mixed semantic changes in the same commit as low-risk extraction
- Each slice requires:
  - targeted tests
  - one independent audit pass
  - merge only after audit findings are resolved or explicitly deferred
