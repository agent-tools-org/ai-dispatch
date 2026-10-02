# aid Roadmap

Updated 2026-10-03 after unreleased route and settlement fixes.
The current release remains v10.50.0 (`89e09057`).
This document owns execution order and acceptance gates; [CHANGELOG](../CHANGELOG.md)
owns release history. The [takeover inventory](project-status-2026-09-22.md) records
source evidence and verification limits. Release validation is recorded separately from historical audit results.

`ai-board` remains the work-item tracker (`ai-board item list --project ai-dispatch`).
Existing `wi-*` IDs are retained for reconciliation; source implementation status below
does not assert board closure. New slices need board IDs before implementation.

## Current baseline

- The current release is **v10.50.0** (2026-09-30). It carries the first rounds of the
  structural refactor program below, `--read-only` enforcement, and the command-surface
  consolidation (removed `kill`, `output`, top-level `summary`/`finding`/`broadcast`,
  `config add-agent`, `watch --wait`).
- The release candidate passed the full workspace suite on the build box
  (34 test binaries, 3,038 passed, 0 failed) and `scripts/release.sh --dry-run`.
  A recursive `--help` dump (126 pages, default features) was identical before and after
  the CLI module reorganisation. Web-feature help and the live API/Swift gates were not
  re-run for this release.
- Earlier baseline notes (v10.47.x custody and budget work) remain below for reconciliation.

## Structural refactor program (in progress)

Source: a three-lane audit of the 109 non-merge commits before v10.48.0 (execution
lifecycle, agent/model layer, command surface). Each slice must remove code or a concept;
no new layer. Behaviour-preserving moves and semantic fixes land separately.

### Landed in v10.50.0

| Area | Result |
| --- | --- |
| Run configuration | CLI args convert once into `cmd::run::RunArgs` (positional 49-argument bridge deleted); workers rebuild from saved dispatch args; every retry path starts from `RunArgs::for_retry` |
| Command surface | Duplicate/hidden verbs removed; subcommand enums defined once; CLI args grouped in domain-named files; one exhaustive dispatch `match` |
| Agent layer | One bounded probe runner for identity, help, version, model listing and preflight; native OpenCode served by the shared overlay; agy `--model` checked at preflight |
| Routing | `advise` is the ranker for hints; quota terms use the resolved model's group; only installed, enabled, non-excluded routes are recommended; Gemini is excluded when agy is installed |
| Pricing | One price function (override, subscription, catalog figure, vendor-CLI feed, unknown); displays read the enforced price |
| Worktree | One git-status porcelain parser for every reader |
| Read-only | Post-run snapshot comparison of the Git run directory |
| Tests | Guide tests check facts generated from code plus a safety-invariant table; gated-verifier settlement E2E harness |

### Next slices (execution order)

| Order | Slice | Acceptance contract |
| --- | --- | --- |
| 1 (complete locally) | Route: best-of races advise's launchable candidates | No uninstalled or held route races; each racer's resolved model equals the advised model; plan cycling kept |
| 2 (complete locally) | Route: cascade fallback from advise | Fallback is the first launchable advise candidate other than the exhausted agent; Claude and superseded Gemini never selected; a peer below the capability floor is not selected |
| 3 (complete locally) | Pricing: budget scoring uses the price function | Subscription and unknown prices are not "paid"; overrides apply both ways |
| 4 | Run configuration: job file holds runtime state only | Nine runtime fields; old job files still load so running workers can be waited on, stopped and reaped |
| 5 | Read-only snapshot scope | Snapshot from the repository top level; the run repository's HEAD and refs compared; repositories without commits and special files recorded instead of failing |
| 6 | Read-only report location | Auto audit reports written under the task directory for host launches; explicit `--result-file` unchanged |
| 7 | Worktree observation | One capture per settlement step; comparisons computed only where read; per-consumer filters unchanged |
| 8 | Task settlement | Terminal status published once after verification, preservation and continuation selection; see [task settlement design](design/task-settlement.md) |

**Exit per slice:** full remote suite green, an independent audit with test evidence,
guide updated for any contract change, and net production lines not increased.

2026-10-03 progress: slice 1 (`wi-843a`) is complete locally. Production lines
decrease by 35; both full remote suites and strict default/Web lint pass, and
independent review returns SHIP. Slice 2 (`wi-7aad`) is also complete locally: automatic cascades preserve
advice's exact model and declared profile through resolver, quota continuation
and batch handoffs. Production Rust decreases by 10 physical lines; 3,098 default
and 3,130 Web tests pass, with zero failed and 14 existing ignored per suite.
Strict default/Web production lint and independent review pass. [Cascade validation](validation-advised-cascade-2026-10-03.md)
records the source and completed jobs. Slice 3 (`wi-739e`) is complete locally:
budget scoring uses resolved prices and exact overrides, with production Rust
reduced by 2 lines. The final pricing source passes 3,102 default and 3,134 Web
tests, zero failed and 14 existing ignored per suite, strict default/Web lint,
guide validation and independent SHIP. [Pricing validation](validation-resolved-price-scoring-2026-10-03.md)
records the exact source and jobs. Slice 4, runtime-only job state, remains open.

Read-only report preservation (`wi-562b`) and the remote verifier's absolute
launch/wait deadline (`wi-8b23`) also pass their before/after regressions and
independent review. The final candidate passed 3,066 default and 3,098 Web tests,
zero failed and 14 existing ignored per suite. [Validation and audit evidence](validation-route-settlement-2026-10-03.md)
records the exact source, commands, job IDs and limits. Issue #118 retains three
separate source-only configuration/prelaunch gaps; live Drive and M0 API/Swift
gates remain open. These changes do not constitute a release.

### Known limits of the current code

- `--read-only` compares only files under the run directory (from a repository
  subdirectory, that subtree); the run repository's own HEAD, branches and tags are not
  compared; an embedded repository without commits fails the snapshot.
- Batch dependencies and `aid accept` can act on a task whose verification is still
  running; the settlement harness records these as ignored scenarios.
- Retry children of an agent failure (`prepare_retry`) inherit the saved `on_done` hook
  and do not overlay the worker's runtime environment.

## Reconcile the previous queue

| Previous item | Source status at this baseline | Disposition |
| --- | --- | --- |
| `wi-4c47`: agy terminal errors | Implemented; v10.38.0 changelog and buffered watcher fixtures | Retain regression coverage; reconcile board |
| `wi-7b8e`: live probe process ownership | Implemented in `scripts/probe-client-api.sh`; v10.38.0 notes | Re-run with current API baseline |
| `wi-8ffc`: target-project dispatch | Implemented; v10.38.0 notes, further prompt-context fix in v10.47.0 | Protect existing coverage; audit budget path separately |
| `wi-dc6a`: custody without worktrees | Shared-checkout recovery committed in `d7dc85ef` and remotely tested | Review and reconcile board; see evidence below |
| `wi-29bd`: project budget | Target identity enforcement/reporting committed in `d7dc85ef` and remotely tested; init/sync contract remains | Next budget work is slice 3 below |
| `wi-e1a0`: merge conflict attribution | Distinct stash-restore result, durable stash identity, regression tests exist | Acceptance recheck; not an unimplemented feature |
| `wi-5eef`: truthful release dry-run | Recorded in v10.47.0 and covered by release hygiene tests | Remove from implementation queue; keep release gate |
| #152: module-tree migration | run/show/batch subdirectories landed (v10.46.0) | Continue only remaining clusters |

## M0 — Restore a reproducible baseline (P0, next)

1. Reconcile retained IDs and stale active initiatives in ai-board. Do not carry forward
   an active status solely because the August roadmap called it active.
2. Restore the approved rbox test environment and a Python 3.10+ tooling environment.
   Re-run default and Web-enabled Rust suites on the same candidate SHA.
3. Default/Web CI coverage, strict production lint and the background wait race
   are fixed in this patch. Next make the client API probe use a controlled fixture
   rather than relying on an operator's task history.
4. Establish an approved macOS build runner for XcodeGen, both client schemes, and Swift tests.
   Record source SHA, toolchain, commands, exit codes, and log locations.

**Exit:** a reproducible result for every required surface, with failures assigned to
bounded work items. A historical audit or a skipped probe is not a pass.
Baseline-environment work can proceed alongside the two correctness slices in M1.

## M1 — Close dispatch and artifact boundaries (P0/P1)

| Order | Slice | Acceptance contract |
| --- | --- | --- |
| 1 / P0 | Non-worktree artifact custody (`wi-dc6a`) | Rescue preserves recoverable results without committing/amending the principal's active branch or absorbing unrelated staged work. Cover no-HEAD, tagged HEAD, pre-existing dirty files, failure, retry and normal worktree cases. |
| 2 / P0 | Target-project budget identity (`wi-29bd`) | Dispatch A → B uses B's resolved identity for budget checks; A's cap cannot block B and B's cap cannot be bypassed by cwd. Cover no-config targets, linked worktrees, batch and retry. |
| 3 / P1 | Project budget source contract (`wi-29bd`) | Specify precedence and whether edits require sync; test cost/token/window limits and cap removal. Keep declared model-budget preference distinct from enforced spend limits. |
| 4 / P1 | Merge recovery acceptance (`wi-e1a0`) | Branch conflict and stash-restore failure remain distinguishable; recovery identifies the exact retained stash and does not discard local files, including a competing stash. |

**Exit:** each slice has a reproducer, regression coverage, guide updates for any public
contract change, and tests on the reviewed candidate. Infrastructure refusal must remain
separate from agent failure; terminal task status must not imply permission to GC.

### Custody and budget implementation committed — 2026-09-23

The non-worktree settlement slice of `wi-dc6a` is implemented: shared-checkout settlement uses a private-index
recovery ref rather than committing/amending the principal branch. Regression cases
cover real-index preservation, unborn/tagged HEAD, repeated snapshots, renames,
publication failure, and task status. All 12 new regressions and both default/Web
Rust suites pass on the remote builder. [Validation evidence](validation-custody-2026-09-23.md)
records counts, job IDs, and limitations. The 15 strict clippy diagnostics are
resolved by this patch. Live API/Swift gates and board closure
remain outstanding. The local rbox fleet is configured outside the repository.

The target-project identity slice of `wi-29bd` is also implemented. Budget checks,
stored usage aggregation and reporting use the target's persisted project identity.
All 10 new CLI regressions pass; default tests pass and Web tests pass on an unchanged
rerun. The first Web attempt exposed an intermittent background delivery failure,
reproduced and fixed by this patch. [Budget validation](validation-budget-2026-09-23.md)
records both attempts. Next budget work is slice 3: project configuration/sync precedence
and cap removal. Concurrent spend reservation is outside the completed slice.

## M2 — Make client/server delivery routine (P1)

- Decide and document whether release binaries include `web` or ship a separate variant;
  current release builds use default features. Document the matching client setup path.
- Preserve authentication, action-conflict responses, SSE reconnect/restart behavior,
  state/outcome consistency, and latency acceptance across server and Swift client.
- Require the default suite, Web suite, live API probe, both client builds and Swift tests
  on one candidate when the release includes client/API changes.
- Keep dry-run hygiene and exact stash/worktree recovery diagnostics under regression checks.

**Exit:** a clean checkout can build and validate the intended distribution without
personal task history or undocumented local tooling. No release number is promised here.

## M3 — Reduce remaining maintenance cost (P2)

- Continue #152 with rate-limit, worker/PTY and Store mutation boundaries; the
  lifecycle boundary is tracked in the structural refactor program above.
  Keep behavior-preserving extraction separate from semantic fixes.
- #160 adapter consolidation: OpenCode native/overlay and CLI probing are shared as of
  v10.50.0; reassess further sharing against captured protocol fixtures.
- Re-triage [historical UX debt](ux-debt.md): batch dependency lineage (`wi-f4bf`),
  content-hash resume (`wi-1479`), and resource lifecycle (`wi-7804`) remain separate programs.
  Start each with a current reproduction and a narrow acceptance contract.
- Audit remaining blanket lint exceptions, stale knowledge references and tracked
  generated reports; archive useful evidence before deleting artifacts.

**Exit per slice:** fewer duplicated owners/parsers, preserved external behavior, and
scenario coverage. File movement or a lower line count alone is not completion.

## Verification and release process

Rust compilation/tests run through the configured build box, per [CLAUDE.md](../CLAUDE.md).
Do not fall back to local compilation on the operator's Mac.

```bash
# Requires an operator-configured AID_BUILD_BOX and rbox on PATH.
scripts/remote-test.sh --dry-run -- --locked
scripts/remote-test.sh -- --locked
scripts/remote-test.sh -- --locked --features web
# With Python 3.10+, no remote contact or Cargo compilation:
python3 scripts/remote-test-test.py
bash .github/scripts/check-changelog.sh
```

The live probe requires `AID_BIN` pointing to the reviewed Web-enabled binary and
`AID_SRC_DB` pointing to a suitable controlled database. Swift schemes are declared in
[`client/project.yml`](../client/project.yml). These gates require their respective runners;
the custody slice did not run the live probe or Swift targets.

Use `scripts/release.sh` for version/changelog/release commit/tag/push; run its dry-run
first and set `AID_RELEASE_TEST_CMD='scripts/remote-test.sh'`. Do not manually bump or
publish as part of roadmap maintenance. The [August handoff](audit-handoff-2026-08-23.md)
is historical evidence, not the next release candidate definition.
