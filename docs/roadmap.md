# aid Roadmap

Updated 2026-09-23 for the v10.47.1 bugfix release candidate.
The previous released baseline is `ffd07e8e` (v10.47.0); custody and budget
fixes landed in `d7dc85ef`.
This document owns execution order and acceptance gates; [CHANGELOG](../CHANGELOG.md)
owns release history. The [takeover inventory](project-status-2026-09-22.md) records
source evidence and verification limits. Release validation is recorded separately from historical audit results.

`ai-board` remains the work-item tracker (`ai-board item list --project ai-dispatch`).
It was unavailable during this inventory: existing `wi-*` IDs are retained for
reconciliation, and source implementation status below does not assert board closure.
New slices need board IDs before implementation; priorities here are proposed execution order.

## Current baseline

- The previous release is **v10.47.0** (2026-09-15). The patch candidate includes
  shared-checkout custody, target-project budgets, and worker-settlement waiting.
- The authenticated Web API, SSE, and macOS/iPadOS client are implemented; the
  v10.38.0 changelog includes the former August integration train.
- Detached foreground execution, isolated-HOME repair, remote builds, backup,
  status guards, quota visibility, and custody GC have continued evolving through v10.47.0.
- CI now covers Rust build/test/strict clippy for both default and `web` features.
  Swift remains a separate validation gate.
- The patch candidate passes default and Web suites (2,696/2,728 unit tests plus
  121 integration tests each) and strict clippy for both configurations. The
  background delivery/wait race has deterministic coverage. See the
  [bugfix validation](validation-bugfix-2026-09-23.md) for the failed baseline,
  fixture correction, final gates and remaining limits. Live API/Swift gates
  remain unverified.

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

- Continue #152 with rate-limit, worker/PTY, Store mutation, and lifecycle boundaries.
  Keep behavior-preserving extraction separate from semantic fixes.
- Reassess #160 adapter consolidation against real captured protocol fixtures before
  sharing more parsing or completion logic.
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
