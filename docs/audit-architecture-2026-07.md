# Architecture Audit — July 2026

**Date**: 2026-07-03 · **Scope**: full codebase (~81k LOC, 396 files) · **Method**: three parallel read-only audit lenses (layering/coupling, runtime pipeline/state machines, extensibility/config sprawl), every concrete claim independently re-verified against code before an issue was opened. **Outcome**: 26 findings → 26 GitHub issues (#139–#164), 24 fixed in the v9.0.0 campaign, 2 deferred.

## Why this audit happened

A day of incident work kept hitting the same walls: a healthy fix task killed at exactly 30 minutes, a droid parser bug that shipped because no e2e covers the PTY path, a retry that audited the wrong checkout because it lost `--dir`, and a salvage feature that had to be bolted into store mutations because failure transitions had no owner. Each was a symptom; the audit went looking for the diseases.

## Convergent diagnoses (found independently by ≥2 lenses)

1. **Task lifecycle had no owner.** Status writes were scattered across ~21 bare `update_task_status` call sites with no transition validation; cross-cutting concerns (salvage, notifications) could only hook at the DB layer. → Fixed: `TaskStatus::can_transition_to` guard + intent-named transitions + `task_lifecycle` service (#145, #146).
2. **Two execution pipelines, three finalizers.** stdout and PTY paths duplicated ~200 lines with eight behavioral divergences (idle semantics, kill signals, findings propagation, UTF-8 handling, log hygiene); the background worker reimplemented the run lifecycle, silently skipping checklist/hooks/review/audit phases for all batch tasks. → Fixed: #140–#142, #144, #150; full transport unification deferred (see below).
3. **Timeout anarchy.** Fourteen overlapping mechanisms, five different max-duration values, one dead parameter, an activity-blind 30-minute wall clock, and an idle detector that didn't count `Reasoning` as life. → Fixed: #139, #143, #149 (`TimeoutPolicy` resolved once at dispatch).
4. **Config that lies.** Name-only budgets enforced nothing; `[project.agents]` was parsed and read by nobody; `request_limit` displayed but never enforced; typo'd project keys silently dropped. → Fixed: #156, #163; `[project.agents]` handled in #162 sweep scope.
5. **cmd/ as a gravity well.** 152 files, six dependency cycles, lower layers importing CLI command modules for model catalogs and worktree removal. → Fixed: #147, #151; full module-tree migration deferred (#152).

## Issue index

| Issue | Finding | Status |
|---|---|---|
| #139 | fg 30-min activity-blind kill; 3 duration policies | fixed |
| #140–#142 | PTY monitor: findings lost, session re-saved, UTF-8 corruption | fixed |
| #143 | idle ignores Reasoning; hidden 300s fallback | fixed |
| #144 | bg finalize bypasses post_run_lifecycle | fixed |
| #145/#146 | transition guard; salvage out of store | fixed |
| #147 | model_catalog / remove_worktree / hung-recovery out of cmd | fixed |
| #148 | 80-char event truncation; silent kind coercion | fixed |
| #149 | TimeoutPolicy consolidation; dead PTY deadline | fixed |
| #150 | kill-with-grace; PTY log escape stripping | fixed |
| #151 | web/TUI → services; dispatch buckets by domain | fixed |
| #153 | agy read-only hard-fail (adapter policy bug) | fixed |
| #154 | rate-limit cross-contamination (kilo/mimo → opencode) | fixed |
| #155 | retry drops dispatch config; dispatch_args persisted | fixed |
| #156 | budget zero-accrual; token limit enforcement | fixed |
| #157/#158 | watch/wait flag matrix; show no-op flags | fixed |
| #159 | kilo/mimo needs_pty (empirically confirmed) | fixed |
| #161 | PTY e2e test suite (3 tests) | fixed |
| #162 | dead surface sweep (~120 LOC, safe_join, shared-dir leak) | fixed |
| #163 | config/CLI hygiene (deny_unknown_fields, homonyms, renames) | fixed |
| #164 | test layout rule documented | fixed |
| #152 | target module tree (root 79 files, cmd/ split) | **deferred epic** |
| #160 | adapter declarative collapse (−800–1000 LOC staged) | **deferred** (stage plan in issue) |

## Incidents → structure (what the campaign bought)

- 30-minute kill of a healthy task → impossible (deadline now requires concurrent idleness).
- droid OSC bug class → caught by `tests/pty_agent_e2e.rs` before shipping.
- retry losing `--dir`/context → retries rehydrate from persisted `dispatch_args`.
- batch tasks missing verify/hooks/audits → single lifecycle, `LifecycleMode::Background`.
- "FAILED means the work is gone" misreading → live worktree state in `aid show` + `partial-work.md` + WIP salvage commits (shipped v8.105.0, trigger relocated in #146).

## Deferred (tracked, not forgotten)

- **#152 module tree epic**: migrate in the order codex's lens proposed (run/ submodule → show/batch → store repos → root clusters). Each step is an independent PR.
- **#160 adapter collapse**: stages are S-effort each; do stage 1 (shared read-only prefix — partially landed via #153/#154) and the kilo/mimo/qwen overlay absorption first.
- **New findings from the campaign itself** (to file): aid worktree builds have no artifact reclamation (filled the disk mid-campaign); `aid clean` FK-constraint error; `aid run --worktree` cannot resume an existing worktree (continuations must use `aid retry`).

## Source reports

Lens reports archived in aid task records: layering `t-2d560cfa`, runtime `t-9ea3746f`, extensibility `t-3890843d` (each report carries file:line evidence per finding).
