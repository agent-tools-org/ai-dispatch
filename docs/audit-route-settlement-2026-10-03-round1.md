## Findings

1. **P2 — Remote verification can launch after the saved deadline and report success.** `src/remote_build.rs:160` converts the absolute deadline to a duration before `configure` (`src/remote_build.rs:170`). Shim filesystem work and command preparation happen afterward; the runner then acquires `VERIFY_LOCK` at `src/verify.rs:99`, spawns at `src/verify.rs:104`, and starts the duration timer at `src/verify.rs:107`. Nothing rechecks the absolute deadline at that boundary.

   Concrete source counterexample: one second remains when `verify_on` calculates its timeout; setup, scheduling delay, or lock acquisition consumes two seconds; a fast successful command then launches after expiry and receives the original one-second allowance. Its result is `success = true`, rather than the promised no-launch timeout. This violates the saved hard deadline and `default-skills/aid-guide/references/dispatch.md:191`. This is source-only evidence; I did not execute this counterexample.

   **Bounded fix:** carry the deadline into the runner, recompute remaining time after setup and lock acquisition immediately before spawning, return the existing inconclusive timeout result when exhausted, and recompute the remaining allowance for the wait. Preserve the local duration-based 120-second path. Add an isolated regression that holds the existing runner lock past a short deadline, then releases it: assert no launch marker, `timed_out = true`, `success = false`, and no exit code. Also cover a shorter prelaunch delay reducing the command's allowance. No new CLI flag or routing framework is needed.

## Audit Answers

Reviewed clean candidate `b380c181fb8dab098d3910b4c6c05e8aa9d38390` against base `291aa2521028adce4b10860e923d57568ff1d4ed`. Repository files remained unchanged. Supplied KB consultations covered best-of launchable-model routing, backup artifact lifecycle, and remote verification timeout; supplied investigation reports record relevant routing/deadline/custody lessons and no direct backup-specific match.

### (1, MOST IMPORTANT) Invariants across failure, retry, exhaustion, and unknown/default models — FAIL

**Best-of: PASS within the stated bounds.** `src/cmd/run/bestof_plan.rs:21` requests the complete ranked advice list; `src/cmd/run/bestof_plan.rs:31` filters launchable candidates before truncation and cycles complete candidates. The existing advice inventory, eligibility, caller-pool and model-group quota gates supply the exclusions (`src/agent/selection_advice.rs:222`, `src/agent/selection_advice.rs:252`, `src/agent/selection_advice_gate.rs:68`). It does not launch the advisory recommendation fallback or custom candidates.

`src/cmd/run/bestof_plan.rs:46` retains the existing session-switch behavior, replaces even a same-agent parent's model, records AID-resolved provenance and all four profile dimensions, pins known defaults, and forces unknown defaults past later budget/model selection. Resolver precedence supports those choices (`src/agent/run_model.rs:101`). Selected unserved models fail before dispatch (`src/cmd/run/bestof.rs:198`). Candidate IDs and artifact derivation remain on the existing paths; winner selection still requires successful task outcome (`src/cmd/run/bestof.rs:120`). No scoped counterexample found with stable routing/configuration evidence. Ordinary external changes during dispatch remain outside the controlled proof.

**Read-only settlement: PASS.** The violation still records evidence, marks Failed, and preserves checkout contents (`src/cmd/run/read_only.rs:49`). The Stop branch records configured verification as not run, persists the declared result, and returns without ordinary verification or retry dispatch (`src/cmd/run/lifecycle.rs:118`). The outer lifecycle attempts backup afterward (`src/cmd/run/lifecycle.rs:77`). Existing persistence copies report content without changing a failed outcome (`src/cmd/run/output.rs:28`, `src/cmd/run/lifecycle/missing_report.rs:22`). Backup's atomic claim prevents repeated attempts and upload errors remain warning milestones (`src/backup/mod.rs:60`, `src/backup/mod.rs:140`). Normal verification/postprocessing order is unchanged.

**Remote verification: FAIL only for finding 1.** Saved `min(max_duration, hard_cap)`, started-at/created-at fallback, future timestamp clamp, initial expiry, shared deadline across disk repick, and replacement-box persistence are implemented correctly at `src/remote_build.rs:129`, `src/remote_build.rs:145`, and `src/remote_build.rs:179`. Local/no-remote still delegates to the existing 120-second runner. The duration handoff does not preserve the absolute deadline through the final launch boundary.

### (2) Production-boundary regressions and completed-job evidence — PASS, with bounded coverage

My inspection was read-only source/log analysis. I ran no tests, build, lint, guide validator, or remote job. Runtime evidence below comes from supplied completed-job logs. I verified actual test result rows, harness summaries, completion markers, and available exit sidecars.

The best-of CLI cases execute the compiled binary with synthetic Codex, inspect actual argv and SQLite saved model/profile/provenance, and assert child IDs and winner output (`tests/bestof_advise_e2e.rs:50`, `tests/bestof_advise_e2e.rs:64`). Plan tests exercise real advice and dispatch resolution, including holds, unavailable/auth-failed/weaker routes, cycling, sessions, budget pressure, and known/unknown defaults (`src/cmd/run/bestof_plan.rs:100`).

The three backup regressions capture a real Git baseline, run a synthetic agent, call the production post-run lifecycle, and inspect the actual fake-gws archive, report bytes, failed state, skipped verification, preserved HEAD/index/edit, and one attempt after repeated settlement (`src/cmd/run/lifecycle_read_only_backup_tests.rs:115`, `src/cmd/run/lifecycle_read_only_backup_tests.rs:135`, `src/cmd/run/lifecycle_read_only_backup_tests.rs:189`). They independently reproduce the early-return defect; they do not manually pre-persist the report.

The nine deadline regressions exercise saved-policy arithmetic, the production runner, short-command timeout, exhausted no-launch, future timestamps, disk repick and persistence, and local/legacy fallback (`src/remote_build/deadline_tests.rs:36`). The above-120 case combines arithmetic with a fast command; it does not execute a command lasting over 120 seconds. Timing cases use isolated child test processes. None covers finding 1's prelaunch interval.

| Supplied log | Completed job | Verified result |
|---|---|---|
| `candidate-default.log:4578` | `a74fb30d027842d38d56d13cbf5c70b2` | Exit 0; 34 top-level binaries; 3,062 passed, 0 failed, 14 ignored; sidecar 0 |
| `candidate-web.log:4610` | `9120e40e4bdf48ebaa1041d792d04f71` | Exit 0; 34 top-level binaries; 3,094 passed, 0 failed, 14 ignored; sidecar 0 |
| `candidate-lint.log:17` | `328513dd2f7e494291efb86f85dabe46` | Exit 0; two successful dev-profile finishes; sidecar 0 |
| `backup-result-before.log:110` | `8141f79d62ca4d09b0ed6f06f2df3385` | Exit 101; 1 passed, 2 failed, 0 ignored; sidecar 101 |
| `backup-result-full.log:4668` | `7ba127558b9a4f5cb16ef539d4b2a5b7` | Exit 0; 33 top-level binaries; 3,040 passed, 0 failed, 14 ignored; no exit sidecar supplied |
| `backup118-baseline.log:208` | `0de4373ecf774d0abda3460645319887` | Exit 0; 22 passed, 0 failed, 0 ignored; sidecar 0 |

Top-level totals exclude one nested subprocess summary in each full-suite log. Default's main harness reports 2,890 passes; Web reports 2,922; both add 172 integration passes. Explicit passing rows include all three backup regressions, nine deadline regressions, nine plan regressions, and five controlled CLI cases. The before-fix failures occur at the missing `result.md` read (`backup-result-before.log:94`); that log also contains a synthetic upload-copy error in one case, so it is not clean upload-success evidence. The 22 older backup tests passed without reproducing report loss.

The final logs show compilation and completed tests from the synced worktree, but contain no candidate SHA or command/feature arguments. Attribution to clean, unchanged `b380c181`, and identification of the lint invocations, rely on the supplied source provenance. The lint log independently proves successful completion and no emitted diagnostics; it does not independently identify exact check/clippy flags.

Each final suite retains 11 main-harness ignores and three settlement E2E ignores. The main ignores comprise eight subprocess helper cases, a live price-feed probe, cached GitButler detection, and a manual TUI performance probe. The settlement ignores concern existing provisional terminal status, early acceptance, and batch dependency ordering. These are separately reported and are not attributed to this patch.

The earlier Web job `233c0009160046f1b25c2f1901e570c2` exited 101 with 2,921 passed, one failed, and 11 ignored (`40ef15fa-candidate-web.log:4419`). The failed guide-facts assertion identified the undocumented port flag. The final guide-facts rows pass in both suites. Task `t-087492b6`'s wrapper timeout does not negate completed job `7ba127558b9a4f5cb16ef539d4b2a5b7`'s exit 0 or turn the wrapper result into a pass.

### (3) Implementation, guide, production delta, and limitations — FAIL for finding 1; otherwise consistent

The authoritative guide accurately describes best-of and read-only settlement. Its absolute remote deadline/no-launch promise exceeds the current runner boundary, as finding 1 explains. No dependency, public flag, status layer, or unrelated production subsystem was added. The new best-of plan module is a bounded extraction. Obsolete ranker/model-tier helpers were removed.

The only diff from `40ef15fa` to `b380c181` documents existing `--port` and default 8080 in `default-skills/aid-guide/references/command-index.md:76`; `src/cli/admin_args.rs:115` already defines it. No allowlist addition or test skip accompanies that follow-up.

Independently counted physical production lines, excluding test-only files and trailing test sections: best-of scope **921 → 886 (-35)**, matching its report; lifecycle **674 → 679 (+5)**; remote-build **182 → 216 (+34)**. Combined production delta is **+4**, not -35. The -35 claim is specifically best-of. The lifecycle file's existing size-limit breach remains; this patch adds five lines to it.

Earlier pending-validation and missing-clippy prose in `docs/investigation-remote-verify-deadline-2026-10-02.md:49`, `docs/investigation-bestof-advise-2026-10-02.md:217`, and `docs/roadmap.md:60` predates the supplied final jobs and needs evidence reconciliation, not classification as a runtime failure. Also correct the older backup report's 3,041-pass/34-binary aggregate at `docs/investigation-backup-result-2026-10-02.md:74` to 3,040 top-level passes across 33 binaries; its additional summary belongs to a nested helper.

The three separate backup findings remain outside this fix: configuration discovery, prelaunch backup admission, and malformed configuration (`docs/investigation-backup118-2026-10-02.md:15`). Synthetic CLI/archive tests establish neither live authenticated Google Drive compatibility nor API/Swift validation. Separately supplied guide validation and diff-check results were not rerun here. No installed-binary upgrade or release is established.

## Verdict

**FIX.** Best-of and read-only settlement pass the scoped review. Repair the remote deadline handoff and add the prelaunch-expiry regression before shipping the combined candidate. Reconcile the bounded evidence prose and counts alongside that fix.
