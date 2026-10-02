## Findings

1. **Low — the guide overstates protection after explicit batch fallback exhaustion.** `default-skills/aid-guide/references/collaboration.md:38` says an exhausted saved list is never replaced by automatic advice; `references/dispatch.md:532–533` makes the same promise for explicit batch fallback lists. The guard is narrower: `src/cmd/batch/dispatch_support.rs:201–204` stops failed-task auto-fallback when the current specification declares a fallback and the saved cascade is empty. In contrast, `src/cmd/batch/retry.rs:78–83` selects automatic advice for a held agent with an empty saved cascade, without distinguishing an exhausted explicit list. For example, retrying a held failed task whose explicit fallback was already consumed can select another advised peer. Prelaunch held resolution also proceeds to advice after walking the explicit list (`src/cmd/run/dispatch_resolve_held.rs:78–98`). Narrow the guide promise to failed-task batch auto-fallback; preserving the existing runtime contract requires no production change. The completed exhaustion regression exercises that narrower path (`tests/advised_cascade_e2e.rs:218–236`, default log lines 4269/4276), not stored `batch retry` exhaustion. This finding is source-traced, not a newly executed reproduction.

**Overall: FIX**, for the guide wording. No blocking production regression was identified.

## Scope and verdicts

Candidate: `958acd3f2fa445ac70712a463c6d5c077724789e`. Base: `724395fd15274a1c283ad6e3d869c4cfbd70c284`. The checkout matches the candidate and has no tracked changes. Review covered the complete diff, the old automatic readers, current advice/model/launch machinery, extracted lifecycle code, regression fixtures, and complete final log contents. No Rust compilation or tests were started by this audit.

KB consulted: the routing lookup matched `failover-granularity-must-match-metering` and `aid-quota-limited-is-per-model-family`. Their relevant constraints are model-group health and preventing later validation from undoing a selected route.

| Question | Verdict | Basis |
| --- | --- | --- |
| 1. Automatic advice route and launch fidelity | **PASS** | All automatic execution readers retain complete candidate arguments; selected models/defaults survive resolution and actual CLI handoff. |
| 2. Explicit overrides, gating and boundary regressions | **PASS** | Existing override semantics remain, and resolver/CLI tests exercise the material launch boundaries. Exhaustion guarantees have the narrower scope described in Finding 1. |
| 3. Bounded scope, accurate guide and completed validation | **FAIL** | Scope and completed compiler/test evidence pass; guide accuracy fails Finding 1. Strict physical production delta is **−10**, rather than the supplied counter's −8. |

## Question 1 evidence

`src/cmd/run/advice_route.rs:12–42` builds untruncated advice using all four declared/default profile dimensions, kind, team, prompt and store, then takes the first launchable candidate excluding the exhausted builtin identity and Claude. It never reads advisory `recommended`. Caller detection remains the shared implementation in `src/cmd/advise.rs:39–52` and `src/session.rs:12–14,46–69`; same-pool capability exclusion is in `src/agent/selection_advice_gate.rs:118–129`.

Application switches agents first, copies the exact model, marks `Advised`, forces unknown defaults, and materializes all four profile dimensions (`advice_route.rs:45–53`). Cross-agent switching clears the previous model/session/provenance (`src/cmd/run/post.rs:287–294`). Kind and team remain on the cloned arguments.

The six former reader locations are accounted for:

- The eager automatic string cascade in batch argument conversion is deleted (`src/cmd/batch/args.rs:35–46`). Config display now uses advice solely for its display hint (`src/cmd/config_display.rs:154–167`).
- Both prelaunch agent-held and model-held substitution use the shared candidate/application path (`dispatch_resolve_held.rs:20–59,68–101`).
- Stored batch retry selects and applies a candidate on restored arguments (`batch/retry.rs:72–91`). Failed batch auto-fallback returns complete arguments and passes them directly to `run` (`batch/dispatch_support.rs:190–227,110–135`).
- Quota continuation marks the actual refused route, selects advice, applies the model and links/inherits the parent target (`src/cmd/run/quota_continuation.rs:14–37`). Both lifecycle modes reach this same branch (`src/cmd/run/lifecycle/phases.rs:174–209`; `src/background_lifecycle.rs:28–46`).

Explicit models precede subsequent budget/default routing, while forced unknown defaults precede everything (`src/agent/run_model.rs:99–109`). Advised routes are hold-checked and exempt from silent healthy-family substitution (`dispatch_resolve.rs:143,187–216`). Model-held substitution copies the advised model, and validation retains unknown evidence but rejects definitive mismatches (`dispatch_resolve_held.rs:49–59`; `src/agent/model_validation.rs:177–198`). Saved arguments preserve that provenance (`src/cmd/run/dispatch_prepare.rs:213–218`; `args_retry.rs:34–39`). Worker reconstruction and launch use the saved effective model (`args.rs:124–139`; `src/background_launch.rs:49–66`). Worker spawning inherits caller environment (`src/background_spawn.rs:18–28`). No string-only automatic execution cascade remains. The existing same-agent model-unavailable self-heal explicitly resets to a default retry (`model_selfheal.rs:45–55`); it is a separate continuation contract.

Actual evidence includes 10/10 CLI regressions in each final suite, covering quota continuation, both known and unknown defaults under budget pressure, saved batch retry, failed batch fallback, caller floor, empty automatic sets, model rejection, and default-profile persistence. Nine advice-route tests and five saved batch retry tests also passed in each suite (default log lines 1460–1478 and 982–986). Resolver tests include both held phases, background urgency and selected-model holds.

## Question 2 evidence

Explicit lists retain order, whole-list custom-name validation and remaining entries (`dispatch_resolve_held.rs:74–119`; `batch/dispatch_support.rs:239–282`). Explicit Gemini is resolver-tested despite installed agy (`src/cmd/run/advice_route_tests.rs:167–194`); explicit Claude remains accepted by the ordinary resolver rather than automatic advice. Existing healthy-family escape and substituted-pin preservation remain (`dispatch_resolve.rs:197–245`; `dispatch_resolve_held.rs:13–18,49–58`). Their three resolver pin regressions pass at default log lines 1656–1658, and held/list/custom/session regressions pass at lines 1648–1680.

Advice eligibility gates disabled, unavailable, auth-failed, below-floor, weaker-caller-pool and superseded routes (`src/agent/selection_advice.rs:221–279`). Launchability rejects the selected model's held quota even for background urgency (`selection_advice_gate.rs:68–71`; `selection_quota.rs:67–85`). Newly held advised routes fail before launch (`advice_route.rs:56–67`). No-peer CLI regressions assert no task/agent launch; model mismatch and caller-floor CLI regressions also assert no launch (`tests/advised_cascade_e2e.rs:110–188`). These are behavioral boundaries, not only helper mirrors.

The existing explicitly selected background primary hold policy remains (`dispatch_resolve.rs:145–151`). It does not permit a held automatic peer. Two inherited background quota tests in `src/background/tests.rs:914–946` only inspect fixture fields; their passing names are not evidence of lifecycle behavior. No dedicated new test of quota continuation specifically in `LifecycleMode::Background` was identified; both modes share the independently traced continuation branch, and the CLI regressions execute real worker handoffs.

Runtime environment is reconstructed from the current batch specification, forwarded names and shared directory (`batch/dispatch_support.rs:115–117`; `batch/args.rs:138–153`). Persisted dispatch JSON removes values (`run/args.rs:100–108`). The actual child-environment/redaction regression passes in both final suites (`tests/advised_cascade_e2e.rs:192–215`).

## Question 3 and validation evidence

The category-matrix picker and all compatibility exports are removed. No dependency, public flag, ignored-test annotation or package-version change appears in the diff; no production `unwrap()` was added. All changed Rust files are at most 299 physical lines. All 12 moved lifecycle-step bodies and opening lifecycle ordering match base after whitespace normalization. The outer backup-on-settlement wrapper already exists at base and remains unchanged (`src/cmd/run/lifecycle.rs:61–87`). Existing long orchestration/validation functions remain over 50 lines; the slice does not eliminate that inherited limitation.

The supplied `production-lines.py:29–33` reports −8 because it excludes trailing test modules but retains the new test-only attribute/import at `src/cmd/run/bestof_plan.rs:6–7`. Excluding test-only items consistently gives **−10 physical production lines**. Headers, production imports, blanks and both sides of moves are counted. The nonpositive requirement passes either count. Static diff review identified no added real credentials; runtime environment-value redaction has actual CLI evidence. This is not a publication-artifact or live deployment security assessment.

| Completed log | Job identifier | Actual result |
| --- | --- | --- |
| `cascade-sha-default.log` | `e721a739cdde4c89b8a941f1dc5eb3f4` | `cargo test --workspace --locked`: **3098 passed, 0 failed, 14 ignored, 35 binaries**; exit 0 at line 4625. |
| `cascade-sha-web.log` | `3d9b504079a54a0db4565c9f054af3ac` | `cargo test --workspace --locked --features web`: **3130 passed, 0 failed, 14 ignored, 35 binaries**; exit 0 at line 4657. |
| `cascade-sha-lint.log` | `03698552989a4b71a7da211541e76ba8` | Both `cargo clippy --locked -- -D warnings` and `cargo clippy --locked --features web -- -D warnings` completed successfully; exit 0 at line 23. |

All three final logs identify the exact candidate SHA, tracked-clean source and Rust/Cargo 1.99.0. Counts independently sum the 35 outer zero-filter summaries; the nested 1-pass filtered summary at line 682 of each suite is excluded. Test-target warnings exist; they do not invalidate the strict production clippy results. The 14 ignores are unchanged.

Baseline job `d2b6da6ec44c44708a8bf1e2b1a7b6ea` completed with exit 101: **0 passed, 1 failed, 9 filtered**, actual `gpt-5.6-luna` versus expected `gpt-6-sol`. That log lacks a source-SHA/clean marker, so its exact unchanged-base production provenance remains unverified from the log alone.

The standalone guide validator claim remains unverified: no completed validator output was found in the supplied logs, and this audit's read-only invocation could not import `yaml`. Both completed suites do run the official-guide unit/CLI tests. The earlier probe-timeout assertion failure's cause remains unproved; its only changed byte sequence enhances test diagnostics, and the final suites pass. Draft verification/−4-LOC paragraphs in `docs/investigation-advised-cascade-2026-10-03.md:36,50–60` were not used as current evidence.

Remaining work: narrow Finding 1's guide promise, replace provisional investigation paragraphs with completed evidence and the consistent count, and obtain standalone guide-validator output in its configured environment. No candidate compiler/test job remains pending in the supplied evidence. This verdict applies only to the audited SHA; later code changes need corresponding validation.
