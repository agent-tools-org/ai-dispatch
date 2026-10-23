# aid Roadmap

Updated 2026-10-20 after v11.3.0 (`ec25a4f5`).
This document owns execution order and acceptance gates; [CHANGELOG](../CHANGELOG.md)
owns release history. The [takeover inventory](project-status-2026-09-22.md) records
source evidence and verification limits. Release validation is recorded separately from historical audit results.

`ai-board` remains the work-item tracker (`ai-board item list --project ai-dispatch`).
Existing `wi-*` IDs are retained for reconciliation; source implementation status below
does not assert board closure. New slices need board IDs before implementation.

## Current baseline

- The current release is **v11.5.0** (2026-10-20): Codex keeps the user's provider,
  agent-scoped failure classification, and removal of the Gemini CLI agent; see that section
  below. v11.4.0 (2026-10-20): truthful outcomes under concurrency and long runs. v11.3.0 (2026-10-16): Claude API account routes. v11.2.0
  (2026-10-16) moved Codex to `codex app-server` with mid-run steering; v11.1.0
  (2026-10-14) made input commands report what aid actually did. See the CHANGELOG.
- v11.0.0 (2026-10-14): failure truthfulness; see
  "Failure truthfulness" below. The release gate passed the full workspace suite on the
  build box (38 test binaries, 3,442 passed, 0 failed, 15 ignored). The native Codex
  permission test was not run: no build box had the Codex CLI installed.
- v10.52.1 (2026-10-09): third-party capability evidence
  and detached feed refresh; see "Third-party capability evidence" below.
- v10.51.0 (2026-10-08): quota, cost and isolation fixes from
  [the magpie comparison](investigation-magpie-absorption.md); see the section below.
  The release candidate passed the full `--bin aid` suite on the build box (2,982 passed,
  0 failed, 11 ignored), `aid_guide_e2e`/`init_e2e`, and `scripts/release.sh --dry-run`.
- v10.50.0 (2026-09-30) carried the first rounds of the
  structural refactor program below, `--read-only` enforcement, and the command-surface
  consolidation (removed `kill`, `output`, top-level `summary`/`finding`/`broadcast`,
  `config add-agent`, `watch --wait`).
- The release candidate passed the full workspace suite on the build box
  (34 test binaries, 3,038 passed, 0 failed) and `scripts/release.sh --dry-run`.
  A recursive `--help` dump (126 pages, default features) was identical before and after
  the CLI module reorganisation. Web-feature help and the live API/Swift gates were not
  re-run for this release.
- Earlier baseline notes (v10.47.x custody and budget work) remain below for reconciliation.

## Next release (in progress)

| Slice | Board | Acceptance contract |
| --- | --- | --- |
| Codex custom provider as a route (P2) — done | wi-3856 | A task launched with a user-selected custom provider records it as its own route identity; quota holds, advice and retry address that route, not `codex/openai`; read-only provenance still proves the restrictive profile |
| Typed pre-command results from rbox — done | wi-1ad2 | A remote verify that never started its command is classified from rbox's typed result, not from log text |
| Listing of unrecognised failures — done (reduced) | wi-1ad2 | `aid agent failures` lists recorded `unclassified_failure` markers, grouped and masked by a fail-closed allowlist; it dispatches no agent and never sets or clears a hold. Advice comes from piping `--json` to the tool-free `aid classify` |

| Code-quality round — done (local) | — | No crate-wide `dead_code` allowance; zero compiler warnings in default/web check and test builds (chrono deprecations excepted); no source file over 300 lines; duplicated display/Git helpers have one owner; output strings unchanged |
| Bare `--verify` honours the project verifier — done | — | `--verify` without a value uses the project's declared verify command (auto-detection only when none is declared); every verify run persists a started/finished event with command, directory, local HEAD, exit code and output excerpt; NoOp is read only from the agent's final message, never tool output |

P2 records only the route name (`codex-provider:<id>`) and refuses a launch, retry or resume when
`config.toml` now selects a different route; it stores no settings snapshot. Known limits: such
tasks are priced at OpenAI rates; `aid agent quota` shows the ChatGPT plan under `codex`; tasks
recorded by v11.5.0 carry no route and cannot be told apart from default-route tasks.

Typed rbox results need rbox 0.7.0 or later (`rbox exec --result-file`); the cargo shim and
`scripts/remote-test.sh` pass the flag only when the rbox they call lists it, and fall back to the
text rule otherwise. Receipts only refine that rule: any started-job line in the output still
blocks an infrastructure classification and re-pick. Known limit: rbox reports local argument
errors (for example a zero job count) as `launch_error`, so such a verify spends its one re-pick.

Fixed on main from that list (2026-10-22), each with a regression test and an independent audit
where it was a bug fix:
- `aid accept` and `aid gc --task` handle terminal tasks with no recorded worktree (read-only and
  in-place runs): acceptance records no head or manifest; collection touches no repository and
  reclaims only eligible fallback Cargo targets. A head-less acceptance never authorizes deleting
  a recorded worktree. A task whose worktree directory is gone was already accepted when its final
  commit is reachable; an unreachable final commit is still refused (custody unresolved).
- `scripts/remote-test.sh` reports a connection lost after the remote job started (a logged
  job id, or a typed receipt other than `not_started` without a job) as such, names
  `rbox log <box> <job>` when the id is known, and keeps rbox's exit status.
- The `codex-provider:` route name has one owner (`src/agent/codex/provider.rs`); five P2 tests
  were strengthened so they fail with the production change reverted.
- The post-verify "dependencies weren't installed" hint fires only for ecosystems with a separate
  install step whose install directory is missing; Cargo- and Go-only worktrees never get it.

The failure slice was cut down after two audit FIX rounds: a model review dispatched as an
agent ran a tool-capable process on provider stderr that an attacker can influence, and a
denylist redactor kept missing credential shapes. The listing masks everything except short
words, short codes, versions and lowercase hostnames; values that are themselves such words are
kept unless a credential name precedes them on the same line (documented in the guide).
Follow-ups: markers still store the raw excerpt (redact at write time); compact JSON error
bodies are masked whole, losing their error type.

Still open: `pty_idle_hang_e2e` failed once under full-suite load and passed alone and in later
full runs. The missing-dependency hint still fires for hoisted npm workspaces, Yarn PnP and
Python environments outside the project. `aid ask` and MCP `ask_text` run as ordinary agent tasks
in the caller's project (project rules, memory, team knowledge, default skills, full agent
permissions); a review of their scope is open. Codex executors could not reach the build box from
their sandbox in most runs this round, so their remote checks were run by the dispatcher.

Carried follow-ups: Claude read-only under Claude Code's own sandbox (would let the v11.4.0
read-only slice cover Claude); snapshotting ignored files written by advisory agents; rbox
ensure-stamp mismatches seen on two boxes.

## Provider carry-over, scoped classification, Gemini removal (v11.5.0)

| Slice | Board | Acceptance contract |
| --- | --- | --- |
| Codex keeps the user's provider (P1) | wi-3856 | `model_provider` and the selected `[model_providers.<id>]` table reach exec, app-server and exec fallback identically as overrides; inline credentials refuse the launch; route identity is unchanged |
| Remove Gemini CLI | wi-08c8 | The built-in agent and its signatures are deleted; naming it fails with a migration hint; stale settings are ignored; stored history with that agent name still loads and displays. Qwen Code stays, with its own quota signatures and reset parsing |
| Error classification, slice 1 | wi-1ad2 | Hold classification uses only the marker's own agent; recovery times come only from reset phrases or typed fields; holds over 24 h or needing a human require a provider-attributed source; Codex app-server typed errors classify directly; unrecognised failures are recorded for review |

Integration found two cross-slice regressions that per-slice `--bin aid` verifies missed, both
fixed before release: a failed PTY run's quota refusal no longer continued to the next route
(now it continues with a transient model hold), and the Codex resume end-to-end fixtures read
an empty Codex home after the provider carry-over. The release candidate passed the full
workspace suite on the build box with default features (44 test binaries, 3,750 passed,
0 failed, 16 ignored) and with `--features web` (3,783 passed, 0 failed, 16 ignored);
`scripts/release.sh` re-ran the suite before tagging.

## Truthful outcomes under concurrency and long runs (v11.4.0)

Source: [failed-task review 2026-10-20](investigation-failed-tasks-20261020.md).

| Slice | Acceptance contract |
| --- | --- |
| A. Read-only concurrent change | A launch whose recorded provenance proves aid's restrictive native profile reports post-run checkout differences as a concurrent-change warning (baseline/current HEAD, paths); unenforced or unproven launches still fail |
| B. Claude mid-run input | Claude runs with streaming input; `aid steer`/`reply` record acceptance only when Claude replays the message; tasks without the transport still refuse |
| W. Worker-owned wrap-up | The worker requests wrap-up at 90 % of the deadline it enforces (reaper as backup, one request); read-only tasks get a report-only request |
| C. Usage survives a kill | Usage checkpoints persist at ingestion; worker timeout, reaper and stop settle from the latest checkpoint; Claude cumulative cost is never summed |
| D. Infrastructure verify outcome | A remote verify that failed before the command ran records the infrastructure outcome, not a failed verify; an auto-picked box retries once on another box within the deadline |
| E. Diagnostics kept | The reaper appends to agent stderr; worker stderr goes to a task log; preparation errors after the pending row settle with their own text |

All six slices landed with independent audits. Integration found two cross-slice defects
before release, both fixed: the idle watchdog's first activity probe could outlast a short
idle window (now the window starts at the first sample), and a production path used a
dev-only crate. The release candidate passed the full workspace suite on the build box
(42 test binaries, 3,707 passed, 0 failed, 16 ignored).

Follow-ups moved to "Next release" above.

## Failure truthfulness (v11.0.0, from the 2026-10-13 failed-task review)

[The review](investigation-failed-tasks-20261013.md) found
two state bugs and three handling gaps that cost the most agent time. Order:

| Order | Slice | Acceptance contract |
| --- | --- | --- |
| 1 | Stop wins over the reaper | `aid stop` racing another `aid` process ends `stopped`, never "worker died" |
| 2 | Terminal rows carry duration | Reaper and stop paths write `completed_at` and `duration_ms` |
| 3 | Live child is activity | Merges "Idle timeout during box waits" below: a silent descendant command accruing CPU is not idle |
| 4 | Capacity is transient | Codex "at capacity" / exhausted 503 reconnects: short model hold, one automatic continuation from the salvage commit |
| 5 | Wrap-up before the hard cap | Steer at 90 % of `--timeout`; warn when `--timeout` < 300 s |
| 6 | Codex read-only at launch | Depends on slice "Read-only report location" below |
| 7 | Refusal signatures | opencode 401 auth marker; droid monthly hold; Claude `seven_day` fixture |
| 8 | Capture tasks | No `empty_diff` flag when no repository change is declared |

Release copy: [site and README refresh](site-refresh-2026-10.md).

Slices 1–7 are implemented for v11.0.0, plus a fix found during the work: the caller's
brief and retry feedback are never compressed with auxiliary context
([§8](investigation-failed-tasks-20261013.md)). Slice 8 is open. Each fix has a
regression test and an independent read-only audit.

Known limits found by review (open):
- `aid stop` and the reaper signal recorded worker and descendant PIDs; the descendant
  path does not re-check process identity, so a recycled PID can be signalled.
- The streaming first-token gate (`progress_count() <= 1`) runs before descendant
  activity is considered, so a silent first command can still be killed as idle.
- Generic and Cargo verification output is drained without a bound while surviving
  descendants keep its pipes open.
- Piped idle failures and PTY deadline or idle failures record a duration that includes
  process cleanup (a 3 s kill grace) and output finalization.
- `verify_status` and delivery assessments are observational metadata and can still
  change after a task settles.
- OpenCode's merged PTY stream cannot prove an error line came from the provider: an
  exact 401 text printed by the task plus a nonzero exit can set an auth hold.
- Wrap-up delivery is not crash-safe exactly-once, and an operator steer can race it.
- Time spent waiting for a build-box lock counts against `--timeout`.
- Verification runs in its own process group; `aid stop` signals the worker and agent groups
  only, so a running verify command can outlive the stop and the worktree lock release.
- A Cargo verification timeout kills the direct `cargo` child, not its descendants.
- Verify cleanup sends TERM only to remaining descendants; one that ignores TERM keeps the
  output readers waiting.
- `aid wait` reports "worker exited before settlement completed" when a task reaches its
  hard cap, and counts finished read-only audits as not succeeded (`[VNORESULT]`).
- Auto-verify does not check build-box reachability; it can select an offline box.
- Read-only delivery: aid's own `AID_HOME` writers (`output.md`, transcript) use path
  writes, so `AID_HOME` inside a repository or with planted links is unsupported;
  `aid show --result` ignores the missing-delivery assessment; a later delivery under
  the same task ID does not clear it.
- The native Codex permission-transition test is opt-in and needs a box with the
  Codex CLI installed.
- Batch fallback authorization is per task, and held-route dispatch no longer
  substitutes an advised agent. Worker quota continuation can still select an advised
  peer without a declared fallback, and worker and coordinator can each launch one
  fallback child.

## Third-party capability evidence (v10.52.0, v10.52.1)

Released in v10.52.0; v10.52.1 made the price and score feeds refresh in a detached
download (a refresh thread died with short commands, and `aid advise` never refreshed).

aid assigns no capability scores of its own. The per-CLI capability matrix, the
hand-written `capability` field of catalog rows and the complexity bonus are removed.
Model capability comes from third-party leaderboards served by the price relay at
`/v1/scores.json` (Terminal-Bench 4.0, Epoch ECI and SWE-bench Verified, LMArena
webdev/agent), keyed by the same ids as `/v1/prices.json`. One source per task category,
never blended; a Terminal-Bench row applies only to its exact CLI and effort. A route
with no evidence scores a neutral 6.0 and is never excluded by the capability floor.

Known limits found by review:
- Sources use different 0-10 rescales (Terminal-Bench fixed 0-100, ECI min-max over the
  current snapshot), so the lowest-ECI model rescales to 0 and routes rated by different
  sources are not strictly comparable.
- `harness: measured` is reported whenever a Terminal-Bench row exists, even when the
  task category scored from another source.
- Effort strings must match exactly; a mismatch falls back to the model-level source.
- A model the relay lists under two price ids (e.g. `claude-opus-5-5` and
  `anthropic/claude-opus-5.5`) scores only under the one the scores feed uses.
- Feed refresh (`src/routing/feed_refresh/mod.rs`): the detached `sh` is not reaped, so a
  long-running aid process keeps up to two defunct children until it exits; `.ready`
  is validated then renamed by path (claiming it under a per-process name first would
  close a narrow swap window); the script's `mv` takes no `--` and curl has no
  `--max-filesize`.

## Quota, cost and isolation (v10.51.0)

### Landed

| Area | Result |
| --- | --- |
| Claude quota | Rejected `rate_limit_event` and error results hold the claude route until the stated reset (≤ 8 days); quota evidence for Claude is envelope text only (`Attributable::quota_text`); plain lines still reach auth markers |
| Quota ranking | Headroom penalty reads only windows that cover the requested model; expired windows and sub-24h windows resetting within 60 minutes do not penalise |
| Cost | Codex/Claude completions priced by component (uncached input, cached input, output, cache writes at 1.25× input) when recorded; Claude `total_cost_usd` stays authoritative |
| Usage | OpenCode-format parsers (OpenCode, Kilo, MiMo Code, custom delegates) sum `step_finish` usage; the recorded model is the dominant `modelUsage`/Gemini entry |
| Isolation | Inherited per-CLI config redirect variables (`CONFIG_REDIRECT_VARS`) are removed from host and PTY launches; batch `env_forward` values resolve at launch; job specs are `0600` |
| Tooling | `scripts/remote-test.sh` exits 66 when a filter matches no tests |

### Next (from the same study; none started)

| Slice | Acceptance contract |
| --- | --- |
| Salvage commit identity | `failure_salvage` WIP commits carry the repository's configured identity inside an isolated HOME; a test fails when it falls back to `user@host` |
| Idle timeout during box waits | A verify or agent blocked on the build-box lock is not killed as idle; queue wait does not count toward the verify timeout |
| Model id normalisation | One `canonical_model_id` (case, vendor prefix, date suffix, `[1m]`, `.`≡`-`) used by rating, gate, pricing and probes; Codex probe drops hidden models |
| Context overflow | An overflow failure writes no hold and continues on a model with a larger context instead of a same-model retry |
| Early failure cascade | A non-quota failure in under 30 s with no work cascades without consuming `--retry`; an all-held error lists every route and its reset |
| Vendor signatures | Agent-independent credit refusals (`余额不足`, `insufficient_quota`) and Codex JSON `resets_at` |
| Cost settlement | A Codex ceiling is not failed on a provisional model price; reports prefer the persisted task cost |
| Quota snapshot fields | `credits`, `held`, `span_secs` read from aidbar snapshots |

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
| 4 (complete) | Run configuration: job file holds runtime state only | Fifteen runtime fields (process identities, resolved dir and budget, inline env, worker hook, dirty baseline, recovery timeouts and audit policy); old 50-field job files still load so running workers can be waited on, stopped and reaped |
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
records the exact source and jobs. Slice 4 is complete as of 2026-10-23: the worker reads its
launch prompt from the task row and every other setting from the saved dispatch args.

Read-only report preservation (`wi-562b`) and the remote verifier's absolute
launch/wait deadline (`wi-8b23`) also pass their before/after regressions and
independent review. The final candidate passed 3,066 default and 3,098 Web tests,
zero failed and 14 existing ignored per suite. [Validation and audit evidence](validation-route-settlement-2026-10-03.md)
records the exact source, commands, job IDs and limits. Issue #118 retains three
separate source-only configuration/prelaunch gaps; live Drive and M0 API/Swift
gates remain open. These changes do not constitute a release.

### Code-quality round — 2026-10-22

Behaviour-preserving; no command, flag or output changed.

| Area | Result |
| --- | --- |
| Dead code | `#![allow(dead_code)]` removed from `main.rs` and two agent modules; 116 compiler-reported items deleted, moved under `#[cfg(test)]`, or kept with an item-level allow naming the serde/TOML schema that reads them; web-only store queries and task actions gated by `feature = "web"` |
| Helpers | `ui::format` owns byte/char truncation, token counts and millisecond durations; `workspace::git` owns checked `git -C` output |
| Size | Source files over 300 lines: 28 → 0; production functions over 50 lines: about 100 → 10 |
| Orchestrators | `watch_streaming_with_input`, `run_lifecycle_phases`, `dispatch_with_dependencies`, `run_background_task`, `stats::collect` and `monitor_bridge` each hold their shared locals in one private run-state struct with phase methods; signatures unchanged. Field order mirrors the original locals in reverse, because fields drop in declaration order |
| Test seams | 17 test-only forwarding shims and parallel paths removed (for example `merge_single`, `merge_group`, `Store::update_task_completion`, `watch_streaming`, the PTY `run_agent_process`); their tests call the production entry points |
| Timing tests | Seven load-sensitive stream, steer and guard tests wait on fixture events instead of wall-clock sleeps; stop tests use fake pids above any `pid_max` instead of pids a live process can hold |

Two deliberate output differences: Git failure messages from the worktree live-state reader now
trim stderr like the other readers, and the GC reader passes a non-UTF-8 repository path to Git
unchanged instead of lossily converting it.

`ProcessGuard::wait_with_timeout` joins its timer before reading whether it fired, so a timeout
can no longer be reported as a normal exit, and a failed `wait` no longer triggers the SIGKILL.

Open:

- Remaining functions over 50 lines are dispatch tables or struct literals: `cli::dispatch`,
  `RunArgs::default`, `task_to_run_args`, `insert_skipped_task`, the background run spec.
- About 105 items remain under `#[cfg(test)]`; they are fixtures, isolation guards and
  constructors. A test-only constructor stays a fixture rather than being inlined at each call.
- `aid stop` signals the worker pid only while its command line is still `__run-task <task_id>`,
  checked before the first signal and again before SIGKILL escalation; a reuse between that check
  and the signal is still possible. `ProcessGuard` signals its process group after reaping the
  leader; that group id can be reused only once no member of the group remains.
- Directories over ten files are unchanged (see M3).

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
| #152: module-tree migration | Done: every module lives in a layer directory; `src/main.rs` is the only root file | None |

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

- #152 is done as a relocation. Directories still over ten files: `cmd` (148),
  `cmd/run` (111), `agent` (105), `cli` (25), `cmd/show` (22), `store` (22),
  `cmd/batch` (21), `types` (20), `batch` (16), `store/queries` (11). Split them only
  alongside a behaviour change in that area; keep extraction separate from semantic fixes.
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
