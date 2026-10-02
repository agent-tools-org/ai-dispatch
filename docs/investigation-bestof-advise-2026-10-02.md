KB consulted: `best-of advise resolved model` returned model-family quota and failover-granularity lessons. A selected model must retain its metered quota group at launch.

# Best-of advice routes — structural slice 1 (`wi-843a`)

Base: `291aa2521028adce4b10860e923d57568ff1d4ed`.
Implementation: `6ea5921dbaa8b602c4ab4392d817c4b106dd6dbc`.
Final combined source: `113584e2ad0b2adf7a2d0dd1dbce8cf00fdea6f6`.

## Reproduction and repair

Previously, best-of called `budget_ranked_agents`, which returned agent kinds and
checked disabled state without enforcing advice inventory, eligibility or
model-group holds. Same-agent children retained the parent's explicit model
when advice selected a different model.

Best-of now uses `cmd::advise::build_report` with the declared profile, kind,
team, store and detected caller, with `top=0`. It filters complete ranked builtin
candidates through `AdviceCandidate::launchable`, takes the top N and cycles
complete candidates. `recommended` does not authorize launch; custom advice's
separate scoring scale is not mixed into builtin ranking.

Every child stores all four profile dimensions and the advised model with
`AidResolved` provenance. Known CLI defaults are pinned. Unknown defaults clear
the parent's model and reuse the existing forced-default override to prevent
later budget routing. Served-model validation rejects an advised pin that would
otherwise be dropped for another default. Model-source rendering identifies
materialized pins as `aid-selected model`.

`switch_agent` retains same-agent sessions and clears cross-agent sessions.
Candidate IDs, output/result paths, metric selection and winner finalization
retain their existing contracts. The old ranker and unused model-tier helpers
are deleted; historical score inputs remain test fixtures only.

## Controlled regressions

`tests/bestof_advise_e2e.rs` installs a synthetic Codex executable in an isolated
PATH and AID home. It compares advice with actual launched argv, deliberately
supplies a conflicting parent model, and inspects requested models, saved
provenance/profile, child IDs and winner artifacts. Five cases cover sticky/CLI
model pins, unknown defaults, no launchable route, an unserved selected model,
and weaker caller-pool exclusion before any launch/task row.

Plan tests cover unavailable/disabled/auth-failed/below-floor routes, agent and
selected-model-group holds, background urgency, unrelated groups, Gemini
supersession, Claude preference, ranked truncation/cycling, sessions, free/cheap
model pins and default/declared profiles. Existing metric/ID/artifact tests remain.

The targeted remote set passed 128 tests: 28 best-of, five controlled CLI,
77 selection, 12 model-info, one guide-facts, two guide E2E and three init E2E.
Both full workspace suites and strict production lint subsequently passed on
the combined source. See [completed validation](validation-route-settlement-2026-10-03.md)
for source SHA, job IDs, exact top-level counts, initial failures, independent
review and the existing ignored tests. Guide validation returned `Skill is valid!`.

## Production-line delta

Compared with the base, physical production Rust lines decrease by 35. Counting
includes headers, imports and blanks, excludes test-only files and trailing
`#[cfg(test)]` sections, and counts moved code on both sides.

| Module | Before | After | Delta |
| --- | ---: | ---: | ---: |
| `src/agent/selection.rs` | 96 | 35 | -61 |
| `src/agent/selection_scoring.rs` | 222 | 195 | -27 |
| `src/cmd/run/args.rs` | 273 | 273 | 0 |
| `src/cmd/run/bestof.rs` | 284 | 275 | -9 |
| `src/cmd/run/bestof_plan.rs` | 0 | 60 | +60 |
| `src/cmd/run/dispatch_model_info.rs` | 46 | 48 | +2 |
| **Total** | **921** | **886** | **-35** |

Changed best-of Rust files are at most 300 lines; edited production functions
are at most 50. No production `unwrap()`, dependency or routing layer was added.

## Limits

This slice does not implement advise-driven cascade or price-function scoring.
Controlled executables do not establish live provider behavior. Changes to quota
or CLI configuration during a race remain subject to ordinary dispatch checks.
