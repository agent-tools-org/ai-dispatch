# Lifecycle Cross-Audit Plan

## Objective

Every lifecycle refactor slice must be reviewed from outside the implementation path before merge. The goal is to catch boundary regressions in state transitions, git handling, and task rendering before they compound.

## Audit Workflow Per Slice

1. Implement one slice only
2. Run targeted tests for the touched lifecycle boundary
3. Run an independent audit task against the diff and changed files
4. Resolve findings or record explicit deferrals
5. Merge only after the slice is green and audited

## Required Audit Questions

Each audit must answer these explicitly:

1. Did task status transitions remain valid on success, retry, and failure paths?
2. Did worktree cleanup and result persistence ordering stay correct?
3. Did any rendering path infer semantics from the wrong status field?
4. Did tests cover the edge case that motivated the slice?
5. Did the slice reduce duplication, or only move it?

## Minimum Test Matrix

### Phase 1

- `run_lifecycle_tests`
- `run_audit_tests`
- any dirty-worktree or rescue tests touched by the refactor

### Phase 2

- show rendering tests
- lifecycle tests for delivery assessment updates
- status serialization tests if enums change

### Phase 3+

- rescue/worktree tests
- show summary and audit output tests
- one integration-style run path covering the changed boundary

## Audit Output Format

The audit report for each slice should contain:

- scope
- findings ordered by severity
- open assumptions
- merge recommendation: approve, approve with follow-up, or block

## Merge Gate

Do not merge a slice if any of the following is true:

- dirty-worktree behavior changed without tests
- audit finds a state-transition regression
- file movement leaves two competing implementations in the tree
- a slice mixes extraction and data-model redesign without necessity
