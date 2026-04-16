# Lifecycle Refactor Design

## Current Failure Pattern

Recent fixes cluster around the same boundary: task completion. The current `post_run_lifecycle` path mixes state transitions, worktree inspection, output persistence, retry scheduling, audit integration, and UI-oriented delivery heuristics in one execution chain.

That causes three forms of drift:

1. One bug fix changes multiple concerns at once
2. Task status and verify status carry presentation semantics
3. Git/worktree facts are re-derived independently in multiple places

## Target Architecture

### 1. Completion Coordinator

A thin coordinator should orchestrate phase modules and stop owning business rules directly.

Responsibilities:
- call phases in order
- short-circuit on terminal outcomes
- persist phase result transitions

Non-responsibilities:
- parsing git status
- deciding rescue path exclusions
- inferring hollow output
- running audit subprocess details

### 2. Worktree Settlement Module

Owns all logic that answers:

- Is the worktree dirty?
- Which paths are rescuable?
- Should the agent be retried for uncommitted work?
- Should the task fail due to residual dirtiness?

Inputs:
- task facts
- run args
- worktree directory

Outputs:
- continue
- retry child task id
- failed
- structured snapshot of paths seen

### 3. Delivery Assessment Module

Separate delivery quality from verification.

Proposed model:
- `VerifyStatus`: pending, passed, failed, skipped
- `DeliveryAssessment`: unknown, code_changed, empty_diff, hollow_output, research_output

Rules:
- verify should answer only whether a configured verification step passed
- delivery assessment should answer whether the task produced a substantive deliverable
- show/board rendering should use delivery assessment for output framing

### 4. Side-Effect Modules

Keep post-completion integrations isolated:

- audit runner
- output persistence
- quota rescue
- webhook and hook execution
- worktree cleanup

Each side effect should depend on settled task facts, not recompute them.

## Data Model Direction

### Near Term

- Keep schema-compatible code where practical
- Introduce adapter helpers that map old fields into new view models during transition

### Later

- Add explicit persisted delivery assessment once the rendering rules settle
- Remove UI logic that treats verify status as a proxy for changes made

## Refactor Constraints

- Preserve current CLI behavior in Phase 1
- Avoid workspace-wide formatting churn
- Keep new files within the AI-coding size limits
- Prefer extraction plus delegation before semantic rewrites
