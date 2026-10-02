## Findings

No findings.

**Overall: SHIP.** The two guide changes resolve the first audit's wording finding without changing Rust source or runtime behavior.

## Scope and verdicts

Source SHA: `f4e6014a00c12d971ce4488f103e7c2b87ad1f94`.
Base SHA: `724395fd15274a1c283ad6e3d869c4cfbd70c284`.
First audited SHA: `958acd3f2fa445ac70712a463c6d5c077724789e`.

The candidate checkout is tracked-clean. The complete diff from the first audited SHA contains only the two guide paragraphs; Rust source and test fixtures are identical. This follow-up verifies the prior production conclusions at the routing, resolver, retry, lifecycle and launch boundaries, the guide correction, and complete final log evidence. No Rust compilation or tests were started by this audit.

KB consulted: the routing lookup matched `failover-granularity-must-match-metering` and `aid-quota-limited-is-per-model-family`; both require preserving model-group health through later resolution.

| Question | Verdict | Evidence |
| --- | --- | --- |
| 1. Automatic route and actual launch fidelity | **PASS** | Complete advised candidates reach every former automatic execution reader; model/profile provenance survives resolver and worker handoff. Both full suites include 10/10 actual CLI regressions. |
| 2. Explicit overrides, eligibility and boundary coverage | **PASS** | Order, custom validation, consumption and session clearing remain; ineligible automatic peers cannot rescue an empty launchable set. Resolver and CLI regressions pass. |
| 3. Bounded scope, guide accuracy and completed verification | **PASS** | The guide correction matches source; physical production delta is −10. Full default/Web tests and both strict production clippy commands completed at this exact clean SHA; standalone guide validation passes. |

## Question 1: source and execution evidence

`src/cmd/run/advice_route.rs:12–42` passes all four declared/default profile dimensions, kind, team, prompt and store into untruncated advice, then selects the first launchable candidate excluding the exhausted builtin identity and Claude. Caller detection remains in `src/cmd/advise.rs:39–52`; caller capability gating is in `src/agent/selection_advice_gate.rs:119–129`. Application clears cross-agent state, copies the exact model, sets `Advised`, forces unknown defaults and materializes all four profile dimensions (`advice_route.rs:44–53`; `src/cmd/run/post.rs:287–295`). Kind and team survive on the existing arguments.

Every former reader is accounted for:

- Batch argument conversion removes its eager string-only automatic cascade (`src/cmd/batch/args.rs:35–46`); config display uses advice only as a display hint (`src/cmd/config_display.rs:154–167`).
- Agent-held and model-held prelaunch substitutions select and apply the complete candidate (`src/cmd/run/dispatch_resolve_held.rs:22–58,68–101`).
- Stored batch retry restores arguments before selection/application (`src/cmd/batch/retry.rs:72–91`). Failed-task batch auto-fallback returns complete arguments and hands them to `run` (`src/cmd/batch/dispatch_support.rs:190–227,110–135`).
- Quota continuation marks the refused model route, applies advice and retains parent/target linkage (`src/cmd/run/quota_continuation.rs:14–37`). Foreground and background enter the same continuation branch (`src/cmd/run/lifecycle/phases.rs:174–209`; `src/background_lifecycle.rs:28–46`).

Forced defaults and explicit pins precede later budget/default routing (`src/agent/run_model.rs:99–109`). Advised routes are hold-checked and bypass silent healthy-family replacement (`src/cmd/run/dispatch_resolve.rs:143,187–216`). Model-held substitution preserves the advised pin; unknown served-model evidence keeps it, while definitive mismatches reject dispatch (`dispatch_resolve_held.rs:49–58`; `src/agent/model_validation.rs:177–198`). Effective model/provenance is saved (`src/cmd/run/dispatch_prepare.rs:213–218`), restored for retry/worker (`src/cmd/run/args_retry.rs:33–42`; `src/cmd/run/args.rs:124–138`) and passed into actual agent options (`src/background_launch.rs:49–66`). Worker spawning inherits caller environment (`src/background_spawn.rs:18–28`). No string-only automatic execution cascade remains. The separate same-agent unavailable-model self-heal deliberately retries the default (`src/cmd/run/model_selfheal.rs:45–55`).

Actual CLI assertions compare saved model/profile/kind/team and captured model argv, and reject inherited parent model/session (`tests/common/advised_cascade.rs:217–247`). The quota, known/unknown-default budget-pressure, stored retry and failed batch tests exercise real CLI/worker handoffs (`tests/advised_cascade_e2e.rs:10–108`). All 10 CLI regressions passed in both final suites: default log lines 4265–4277, Web lines 4297–4309. Nine advice-route tests and five advised batch retry tests also passed in each suite (default lines 1455–1479 and 983–987), including both held resolver phases and selected-default persistence.

## Question 2: explicit contracts and guide fix

Explicit lists retain whole-list custom-name validation, order and remaining entries (`src/cmd/run/dispatch_resolve_held.rs:74–118`; `src/cmd/batch/dispatch_support.rs:239–282`). Cross-agent switching clears model, session and provenance (`src/cmd/run/post.rs:287–295`). Explicit Gemini remains resolver-tested with agy installed (`src/cmd/run/advice_route_tests.rs:167–194`); explicit Claude remains an ordinary explicit route. Explicit substituted pins preserve healthy-family escape behavior (`src/cmd/run/dispatch_resolve.rs:197–245`; `dispatch_resolve_held.rs:14–20,49–58`).

Automatic eligibility excludes disabled, missing, auth-failed, below-floor, weaker caller-pool and superseded routes (`src/agent/selection_advice.rs:221–279`). Launchability rejects held selected-model quota even under background urgency (`src/agent/selection_advice_gate.rs:68–71`; `src/agent/selection_quota.rs:67–85`). New holds reject the advised route before launch (`src/cmd/run/advice_route.rs:55–67`). Actual CLI regressions assert no task/agent launch for an empty automatic set, definitive model mismatch and caller-floor exclusion (`tests/advised_cascade_e2e.rs:110–187`); all pass in both jobs listed below.

The corrected paragraphs at `default-skills/aid-guide/references/collaboration.md:37–40` and `references/dispatch.md:531–536` now distinguish the two contracts accurately. Failed-task `auto_fallback` stops when a currently declared explicit fallback has an empty saved cascade (`src/cmd/batch/dispatch_support.rs:201–204`). Explicit `batch retry` can advise a held agent with an empty saved cascade (`src/cmd/batch/retry.rs:78–83`). Prelaunch resolution can also advise after walking an explicit list (`src/cmd/run/dispatch_resolve_held.rs:79–96`). The exhaustion CLI regression proves the failed-task guard, rather than a universal ban on later advice (`tests/advised_cascade_e2e.rs:220–236`; default log line 4270, Web line 4302).

The runtime environment/redaction regression also passes in both suites (`tests/advised_cascade_e2e.rs:190–217`). Saved arguments omit values; failed batch fallback reconstructs runtime environment and forwarded names from the current specification (`src/cmd/run/args.rs:99–105`; `src/cmd/batch/dispatch_support.rs:115–117`).

## Question 3: scope and completed validation

The old category-matrix picker and compatibility exports are removed; no current source reference remains to `coding_fallback_for`, `selection_fallback` or `auto_cascade_for_rate_limited`. No dependency/package-version change, public CLI flag, added ignored-test annotation or production `unwrap()` appears in the bounded diff. Changed Rust files have at most 299 physical lines.

Rerunning the physical production counter gives **−10 lines**, matching `final-production-lines.json`. It counts headers, production imports, blanks and both sides of moves, while excluding dedicated tests, trailing test modules and the test-only import at `src/cmd/run/bestof_plan.rs:6–7`. The first review's −8 counter discrepancy is resolved. Static diff review found no added real credentials; the completed environment redaction test supplies runtime evidence. This does not assess publication artifacts or deployed endpoints.

| Complete log | Job identifier | Actual result |
| --- | --- | --- |
| `cascade-guide-default.log` | `4f5ba5eecdc24afe84a1c229656e3fd8` | `cargo test --workspace --locked`: **3098 passed, 0 failed, 14 ignored, 35 binaries**; exit 0, line 4626. |
| `cascade-guide-web.log` | `1787d55ed12a4c6b8e2787e606c13c92` | `cargo test --workspace --locked --features web`: **3130 passed, 0 failed, 14 ignored, 35 binaries**; exit 0, line 4658. |
| `cascade-guide-lint.log` | `f823220f571e4d66932f292e61ac0508` | Both `cargo clippy --locked -- -D warnings` and `cargo clippy --locked --features web -- -D warnings` finish successfully; exit 0, line 23. |

All three logs identify the exact candidate SHA, tracked-clean source and Rust/Cargo 1.99.0. Counts independently sum the 35 outer zero-filter summaries. Nested 1-pass filtered subprocess summaries at default line 685 and Web line 679 are excluded. The 14 ignores are unchanged. Test-target warnings do not invalidate the strict production clippy results.

`guide-validator.log` contains completed `Skill is valid!` output. An independent read-only invocation of the configured standalone validator also returned `Skill is valid!` and exit 0.

Baseline job `7ee348d65657447bbc1d73f1f2ddcfe0` completed with exit **101**, verified after the required `waitfor` condition. `cascade-baseline-provenance.log:14–19` names the exact base SHA, clean production and only two staged public test fixture paths. Both recorded SHA256 hashes match the final candidate fixtures. The actual targeted result is **0 passed, 1 failed, 9 filtered**, with observed `gpt-5.6-luna` versus expected `gpt-6-sol` at `tests/common/advised_cascade.rs:219`. This establishes unchanged-base production reproducer provenance; the earlier metadata-free log is not needed.

## Scope limits

No candidate compiler/test job remains pending in this evidence. Dedicated quota-continuation E2E output specifically for `LifecycleMode::Background` remains **unverified**; the shared branch is source-traced, and actual worker handoffs are tested. Inherited functions exceeding 50 lines remain. Live vendor behavior and deployment/publication security are outside scope.

The candidate's investigation document retains historical draft validation/count statements (`docs/investigation-advised-cascade-2026-10-03.md:36,50–60`); completed logs and the rerun counter supersede them. Any replacement documentation or other changes outside this SHA remain **unverified** and are not covered by this verdict.

NO_CHANGES_NEEDED: No code or guide correction is required for the audited candidate.
