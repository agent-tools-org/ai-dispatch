## Findings

No findings.

## Audit Answers

Reviewed clean frozen candidate `113584e2ad0b2adf7a2d0dd1dbce8cf00fdea6f6` against `b380c181fb8dab098d3910b4c6c05e8aa9d38390` and original base `291aa2521028adce4b10860e923d57568ff1d4ed`, including the first independent audit. Profile: moderate / premium / normal / standard; debugging. Inspection was source/log-only. **Tests, builds, lint and guide validation were not run by me.** The supplied completed remote logs are executed evidence, independently inspected below. Repository files were not changed.

KB consulted: the supplied absolute-deadline/lock/spawn/verify consultation returned generic latency, liveness and test-integrity lessons. The supplied durable lesson `a-remote-verification-timeout-describes-the-wrapper-not-the-test-job` establishes the distinction between wrapper outcomes and completed jobs, excludes nested summaries from workspace totals, and assigns deadline ownership to the launch boundary.

### (1, MOST IMPORTANT) Absolute deadline through preparation, lock, spawn and wait; exhaustion, cleanup, re-pick and local/skip semantics — PASS

The previous duration handoff is removed. `src/remote_build.rs:160` retains the initial exhausted-deadline/no-launch check; configuration follows at `src/remote_build.rs:164`, and the original absolute deadline reaches the existing environment runner as `VerifyBudget::Deadline` at `src/remote_build.rs:169`. Command construction and environment setup pass that budget unchanged (`src/verify.rs:83`, `src/verify.rs:103`). After acquiring the actual `VERIFY_LOCK`, the runner checks expiry immediately before `ProcessGuard::spawn` (`src/verify.rs:110`, `src/verify.rs:115`, `src/verify.rs:118`). The lock/preparation interval therefore cannot grant a fresh allowance.

`src/verify_budget.rs:27` recomputes remaining wall time at wait, after spawn and output-reader setup. If already exhausted, it kills and reaps the local command (`src/verify_budget.rs:32`); otherwise it uses the existing process-group timeout/cleanup path (`src/process_guard.rs:61`). The final expiry filter at `src/verify_budget.rs:40` discards an exit observed after the deadline even if a delayed timer returned an exit status. `src/verify.rs:122`, `src/verify.rs:135` and `src/verify.rs:142` consequently report timeout, unsuccessful verification and no exit code. No-launch results have the same inconclusive fields (`src/verify_budget.rs:43`).

The saved-policy minimum, started-at/created-at fallback and future-anchor clamp remain at `src/remote_build.rs:173`. One deadline is computed before the first attempt and reused after admission refusal; replacement-box persistence still precedes its attempt (`src/remote_build.rs:132`, `src/remote_build.rs:145`, `src/remote_build.rs:146`). Initial exhaustion avoids command and picker launch. Exhaustion during re-pick prevents the replacement command, while retaining the selected replacement in storage.

Local duration callers explicitly use `VerifyBudget::Duration` (`src/verify.rs:64`), whose allowance still begins in the wait path after lock acquisition (`src/verify_budget.rs:26`). No-remote/legacy tasks retain the existing store-aware local route and 120-second cap (`src/remote_build.rs:129`, `src/verify_cargo.rs:39`). Explicit skip and auto-detected no-project results return before taking the mutex (`src/verify.rs:73`, `src/verify.rs:85`); the remote wrapper's earlier exhaustion check is also retained, rather than reordered around these returns.

No concrete remaining counterexample introduced by this repair was found. The existing provisional terminal-state race, lock responsiveness and cancellation of already-running remote jobs remain outside these guarantees.

### (2) Synchronized before/after regressions and exact-candidate completed-job evidence — PASS

The regressions exercise the production mutex and runner, rather than only helper arithmetic. A readiness channel proves the lock is held before verification begins; the holder observes creation of the task's Cargo shim before starting its delay (`src/remote_build/launch_deadline_tests.rs:35`). The first three cases run in isolated child test processes and require the child's actual successful exit (`src/remote_build/launch_deadline_tests.rs:15`).

- One second of saved allowance followed by a 1.5-second held lock requires timeout and an absent launch marker (`src/remote_build/launch_deadline_tests.rs:64`).
- A three-second allowance with two seconds consumed before launch requires the launched two-second command to time out with captured box output (`src/remote_build/launch_deadline_tests.rs:104`). A reset allowance would let it succeed.
- A local 500-millisecond duration still succeeds on a 200-millisecond command after an 800-millisecond lock delay (`src/remote_build/launch_deadline_tests.rs:143`).
- The fourth case preserves explicit-skip and auto/no-project result fields with an expired internal deadline (`src/remote_build/launch_deadline_tests.rs:170`).

The inspected `launch-repro` worktree is at `b380c181`; its entire diff consists of test-module inclusion, two test-fixture visibility changes, and the first three regressions. Their source matches the final candidate apart from a trailing blank line; old production behavior was unchanged. The fourth case is absent because it depends on the new budget type. The before log's actual failures show successful results where timeout was required: `launch-deadline-before.log:81` reports `success: true`, `timed_out: false`, output `finished`; `launch-deadline-before.log:87` reports the same fields with `chosen-box` after 4.02 seconds. The local case passed (`launch-deadline-before.log:72`). Thus these are executed reproductions of late launch and reset wait allowance. The before log lacks an in-job SHA header; its historical-source attribution is corroborated by the inspected reproduction worktree, unlike the self-identifying final logs.

All final jobs print exact candidate SHA, `SOURCE_TRACKED_CLEAN: yes`, toolchain and CHECK argv before compilation: `final-default.log:13`, `final-web.log:13`, `final-lint.log:12`. Each reports `rustc 1.99.0 (b940084d7 2026-09-28)` and `cargo 1.99.0 (5f94df478 2026-08-27)`.

| Supplied log | Completed job | Actual command and verified result |
| --- | --- | --- |
| `launch-deadline-before.log:94` | `5c39797d599842229f4ff645d96ddc01` | Three selected top-level tests: 1 passed, 2 failed, 0 ignored; completion marker exit 101 at line 98; `.exit` is 101 |
| `final-default.log:4211` | `59c4849f37544817adaa76bd0813c59a` | `cargo test --workspace --locked`: 34 top-level binaries; **3,066 passed, 0 failed, 14 ignored**; completion marker exit 0 at line 4587; `.exit` is 0 |
| `final-web.log:4243` | `a156f4c955854728944922005bb08dcc` | `cargo test --workspace --locked --features web`: 34 top-level binaries; **3,098 passed, 0 failed, 14 ignored**; completion marker exit 0 at line 4619; `.exit` is 0 |
| `final-lint.log:16` | `554783c2d45243939d19042785225aa8` | `cargo clippy --locked -- -D warnings`, then `cargo clippy --locked --features web -- -D warnings`: both dev-profile finishes; completion marker exit 0 at line 23; `.exit` is 0 |

Default's main harness contributes 2,894 passes and Web's 2,926; each adds 172 integration passes. Each full log has 35 summary rows, but only 34 belong to top-level Cargo binaries with zero filtered tests. The extra one-pass subprocess summary at `final-default.log:700` / `final-web.log:687` is filtered and excluded. Nested before-fix failure summaries embedded in child output are likewise excluded from its three-test total.

All four new passing rows occur in both full suites: default at lines 3312, 3315, 3467 and 4183; Web at lines 3313, 3316, 3469 and 3966. The nine retained deadline cases also pass. The full suites therefore cover the new cases, unlike the earlier nine-test-only filtered gate. No new ignored tests were added: the 14 comprise eight subprocess-reporting helpers, the live price-feed probe, cached GitButler detection, manual TUI performance, and three existing settlement scenarios (`final-default.log:4562`). Test compilation emits existing warnings; the successful strict lint commands cover production targets, not all test targets.

These final counts supersede the initial b380 totals of 3,062 / 3,094. Historical wrapper timeouts remain wrapper timeouts; completed remote exits are separate evidence. The above-120-second retained case verifies saved-policy arithmetic and a fast actual command, not a command running longer than 120 seconds.

### (3) Bounded internal API, authoritative guide, scope and retained best-of/read-only contracts — PASS

`VerifyBudget` is a mutually exclusive internal enum in a 54-line module (`src/verify_budget.rs:14`), used by the existing environment runner. The changed verifier remains 295 lines; remote-build is 222 lines and the new regression file is 183 lines. Compared with b380, production edits are limited to that runner, deadline handoff and budget extraction. No dependency, public flag, compatibility shim, new routing/status layer or unrelated source subsystem was added. I preferred evaluating this existing-boundary repair without proposing another abstraction.

The authoritative deadline guide explicitly includes preparation, verifier-lock time, remaining wait allowance and shared re-pick deadline, and distinguishes wrapper timeout from the remote job result (`default-skills/aid-guide/references/dispatch.md:187`). Best-of and read-only guide contracts remain consistent. The existing Web port documentation is a guide correction, not a newly introduced flag (`default-skills/aid-guide/references/command-index.md:76`). Separate guide validation reported `Skill is valid!` and diff-check exit 0; neither was rerun here. Generated guide-facts tests passed in both final suites (`final-default.log:1344`, `final-web.log:1333`).

Best-of still filters complete ranked advice candidates before truncation and cycles full candidates (`src/cmd/run/bestof_plan.rs:21`, `src/cmd/run/bestof_plan.rs:31`); the existing launchability/quota/team gate is retained (`src/agent/selection_advice_gate.rs:68`). Racers preserve all profile dimensions, replace the parent's model, record AID provenance, and force unknown defaults through later resolution (`src/cmd/run/bestof_plan.rs:46`, `src/agent/run_model.rs:101`). Unserved selected models fail before dispatch; winner selection still requires successful outcome (`src/cmd/run/bestof.rs:198`, `src/cmd/run/bestof.rs:120`). Nine plan tests and five compiled CLI cases pass in each final suite (`final-default.log:1503`, `final-default.log:4265`). The CLI fixtures inspect actual argv and saved SQLite model/profile/provenance (`tests/bestof_advise_e2e.rs:64`).

Read-only Stop still records configured verification as not run, persists the report and returns before normal verification/retry; outer settlement then attempts backup (`src/cmd/run/lifecycle.rs:118`, `src/cmd/run/lifecycle.rs:128`, `src/cmd/run/lifecycle.rs:77`). Failure/evidence preservation and backup's atomic single-attempt claim remain (`src/cmd/run/read_only.rs:49`, `src/backup/mod.rs:60`). Three lifecycle/archive regressions pass in each final suite (`final-default.log:1968`, `final-default.log:1994`, `final-default.log:2019`), checking original report bytes, failed status, unrun verification, preserved checkout and one upload attempt (`src/cmd/run/lifecycle_read_only_backup_tests.rs:157`, `src/cmd/run/lifecycle_read_only_backup_tests.rs:189`).

The historical b380 validation remains correctly identified at `docs/validation-route-settlement-2026-10-03.md:5`; the archived first-round verdict remains **FIX** at `docs/audit-route-settlement-2026-10-03-round1.md:64`. As requested, current pending-validation wording can be reconciled after this verdict (`docs/validation-route-settlement-2026-10-03.md:60`, `docs/investigation-verify-launch-deadline-2026-10-03.md:21`, `docs/roadmap.md:60`). Record the final jobs/counts without relabeling round 1, erasing historical wrapper timeouts, or extending best-of's scoped **-35 production lines** to the combined repair.

Issue #118's other three source-only findings, authenticated live Drive compatibility, live provider/API probes, Swift gates, installation and release remain open (`docs/validation-route-settlement-2026-10-03.md:65`). Controlled fixtures and these Rust jobs do not establish those surfaces.

## Verdict

**SHIP** for the scoped integrated source candidate. All three audit questions pass; the first audit's launch-boundary finding is repaired and independently reproduced before the fix. Final-evidence documentation reconciliation remains the stated follow-up; this verdict authorizes no publication or release action.

NO_CHANGES_NEEDED: no repository-code correction is required by this re-audit.
