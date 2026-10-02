## Findings

No findings.

**Overall verdict: SHIP** for structural pricing slice wi-739e at frozen candidate `e469c500c9a715ef9caab5014b4ff99ccb7cd9e0`, relative to base `d45681b71f65566dc68db9d85de4b4089625b726`.

KB consulted: `pricing budget penalty`. The search returned budget-field-override and price-times-volume lessons among broad matches; no direct scoring-resolver match. Source and completed remote output determine this verdict.

## Question 1 — PASS: actual scoring and advice use resolved pricing

The actual path is `advise` → `builtin_candidates` → `resolve_run_model` → `builtin_candidate` → `score_breakdown` → `model_is_paid` → existing `cost::resolve_pricing`. Evidence: `src/agent/selection_advice.rs:135`, `:227`, `:240`; `src/agent/selection_scoring.rs:55`, `:106`; `src/cost/mod.rs:180`.

`score_breakdown` subtracts exactly `3.0` iff budget mode is enabled and the selected model resolves to a known price with positive input **or** output. `None`, zero rates, and absent model do not incur that penalty. Standard/premium advice disables this term through the actual declared-profile context (`selection_advice.rs:141`; `src/types/task_profile.rs:89`).

The unchanged resolver preserves precedence: exact agent/model override → subscription inclusion → catalog figure → exact vendor-CLI feed → unknown (`src/cost/pricing_resolution.rs:13`). Override keys include the agent and lowercase model (`src/cost/mod.rs:196`, `:212`); catalog matching is case-insensitive (`pricing_resolution.rs:47`). Subscription inclusion resolves to zero (`:21`; `src/types/provider.rs:144`, `:147`). Feed pricing remains restricted to Codex/Claude/Gemini/Grok (`pricing_resolution.rs:31`), using exact canonical IDs or aliases (`src/cost/price_feed.rs:51`, `:76`). Feed matching remains case-sensitive; the slice does not broaden it. No network refresh is called by this path.

The four new tests exercise both production boundaries, rather than duplicating the price predicate. `src/agent/selection_price_tests.rs:36` calls `score_breakdown`, creates a real declared profile, calls `advise`, and asserts the returned model, Sticky provenance, pin, installed/eligible state, entire breakdown equality, and total. `:79` checks Free/Cheap/Standard/Premium. The cases prove:

- Catalog-paid, subscription with positive catalog figures, free, and unknown routes (`:101`).
- Zero override removes the catalog-paid penalty (`:121`).
- Uppercase exact positive overrides price free/subscription/unknown routes, for input-only and output-only positivity; neighboring models and another agent remain unpenalized (`:140`).
- Synthetic feed prices its vendor but not reseller or neighboring models, for each positive side separately (`:164`).

All four `agent::selection::selection_price_tests::resolved_price_*` tests actually pass in both full runs: `price-default.log:554`, `:593`, `:613`, `:642`; `price-web.log:543`, `:544`, `:615`, `:655`.

## Question 2 — PASS: remaining behavior and isolation are preserved

The bounded production diff changes only `model_is_paid` and removes its unused catalog import. All score terms and their floating-point evaluation order remain byte-for-byte unchanged: initial quality, budget subtraction, rate-limit subtraction, history, complexity, team, then conditional headroom addition (`src/agent/selection_scoring.rs:101`). Non-budget scoring short-circuits before price resolution (`:106`). The existing bit-exact regression at `src/agent/selection_score_tests.rs:102` passes in both logs (`price-default.log:545`; `price-web.log:546`).

Model resolution/catalog, inventory, eligibility, difficulty floors, caller-pool gating, quota terms, sorting and launchability are unchanged. Relevant boundaries: `src/agent/run_model.rs:99`; `src/model_catalog.rs:178`; `src/model_catalog_resolved.rs:78`; `selection_advice.rs:222`, `:247`, `:257`, `:291`; `src/agent/selection_advice_gate.rs:35`, `:67`, `:119`; `src/agent/selection_quota.rs:13`. Existing floor/pool tests pass at `price-default.log:516–520` and `price-web.log:516–520`; actual advice group-hold, caller-pool and declared-budget tests pass at default `:524`, `:531`, `:537–538` and Web `:524`, `:536–539`. All nine existing scoring tests pass in each suite.

The fixture isolates the aid home and quota cache with guards (`selection_price_tests.rs:17`), resets both thread-local pricing caches before use and after updates, and clears them at each successful test end (`src/cost/mod.rs:27`, `:107`, `:169`). Home/quota overrides restore previous state on drop (`src/paths.rs:156`; `src/live_quota.rs:135`). Pinned inventory bypasses host probing (`src/agent/binary_route.rs:70`), while sticky models survive actual model resolution (`run_model.rs:107`). Prices come from synthetic local files; no live API prices or installed provider CLIs are assumed. Full advice-candidate equality across pricing transitions verifies unchanged non-budget scores and eligibility; cross-budget assertions compare every breakdown term and exclusion codes.

**Observed before/after:** completed baseline job `f4a553fb280f411eb98fe50446f42c00` in `price-baseline-final.log` records the base SHA, `SOURCE_PRODUCTION_CLEAN: yes`, and only the dedicated fixture plus test registration as tracked differences. The completed job checks the registration prefix against HEAD; all other tracked bytes match HEAD. Its fixture SHA-256 is `2826f5fe5b43816b2b2842b2ce1d0556013163042d6f3ad14d8acb3497bec7da`, independently identical to the candidate fixture.

Exact baseline command:

```text
cargo test --workspace --locked --bin aid agent::selection::selection_price_tests::resolved_price_catalog_subscription_unknown_and_free_at_both_boundaries -- --exact
```

It completed with exit **101**, **0 passed / 1 failed / 0 ignored / 2930 filtered**. At `selection_price_tests.rs:85`, actual returned Cursor/composer-2.5/Free penalty was `-3.0`, expected `0.0`; the preceding real advice/model/breakdown assertions had succeeded. The identical test passes on the candidate in both full suites. This baseline is failure evidence, never counted as passing.

## Question 3 — PASS: structural budget and completed verification

Independent physical production-line accounting agrees with `price-production-lines.json`:

| Rust file | Base production lines | Candidate production lines | Delta |
| --- | ---: | ---: | ---: |
| `src/agent/selection_scoring.rs` | 195 | 193 | −2 |
| `src/agent/selection.rs` | 31 | 31 | 0 |
| **Total** | **226** | **224** | **−2** |

The count includes headers, imports and blanks, excludes dedicated tests and trailing test modules, and covers every changed Rust path; there are no moves. Full changed Rust files are 193, 41 and 186 lines. The changed production helper is four lines; the longest new function is 36 lines (`selection_price_tests.rs:36`). No new pricing abstraction, dependency, flag, shim, ignore, public API, release/version/tag change, or production unwrap is introduced.

The added guide paragraph (`default-skills/aid-guide/references/dispatch.md:345`) accurately describes the penalty, override precedence, unknown/subscription behavior and vendor scope. Its Free/Cheap versus Standard/Premium claim is verified against the context constructed by actual `advise`, not just a manually supplied context. `price-guide-validator.log:1` records `Skill is valid!`; generated-guide, guide E2E and init E2E tests also pass in both full runs.

Every accepted candidate job records exact candidate SHA and `SOURCE_TRACKED_CLEAN: yes` before its commands. Default/Web provenance is at log lines 14–18; lint provenance is at lines 12–16. All use `rustc 1.99.0 (b940084d7 2026-09-28)` and `cargo 1.99.0 (5f94df478 2026-08-27)`.

| Completed remote job | Exact command | Observed result |
| --- | --- | --- |
| `7f1de753b7a944eba228949bae6989d0` — `price-default.log` | `cargo test --workspace --locked` | Exit 0 at line 4630; **3102 passed, 0 failed, 14 ignored, 0 filtered** |
| `43faf0bae74b46d48f8818a40acf2e98` — `price-web.log` | `cargo test --workspace --locked --features web` | Exit 0 at line 4662; **3134 passed, 0 failed, 14 ignored, 0 filtered** |
| `bd1ec8975878472bbb38402e5f6f2c96` — `price-lint.log` | `cargo clippy --locked -- -D warnings` | Completed successfully, lines 16–18 |
| Same lint job | `cargo clippy --locked --features web -- -D warnings` | Completed successfully, lines 19–21; whole job exit 0 at line 23 |

Each suite contains 35 top-level test binaries: default unit **2920 passed / 11 ignored** (`price-default.log:4238`), Web unit **2952 passed / 11 ignored** (`price-web.log:4270`), plus 34 integration binaries totaling **182 passed / 3 ignored** per run. Counts exclude nested filtered subprocess summaries at default line 674 (**1 passed / 2930 filtered**) and Web line 658 (**1 passed / 2962 filtered**). Clippy is strict production lint, not test-target lint.

## Residual limits

The 14 existing ignores per suite did not run, including the live price-feed probe. Test builds emit existing warnings; strict production lint passes. Evidence covers deterministic local pricing inputs and the current catalog/resolver, not live vendor rates, remote refresh availability, provider billing, API/Swift, release or deployment. The four pricing regressions pin Sticky routes; broader model-selection/caller/quota behavior is supported by unchanged source and existing passing suites. The investigation document's draft-stage predicted/no-test paragraphs are historical and were not used as current verification evidence.

NO_CHANGES_NEEDED: The frozen pricing slice passes this independent audit; no source edits are required.
