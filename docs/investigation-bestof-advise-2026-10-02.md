KB consulted: `aid-quota-limited-is-per-model-family`; `failover-granularity-must-match-metering`; `a-dispatched-agent-that-reads-the-operators-dispatcher-rules-becomes-a-dispatcher`.

# Best-of advice routes — structural slice 1 (`wi-843a`)

Baseline: `291aa2521028adce4b10860e923d57568ff1d4ed`.

The KB search `best-of advise resolved model`
returned 134 matches, including the model-family quota and metering-granularity
entries above. Their relevant constraint is that a selected model's quota group
must remain the group used at launch.

## Reproduction and change

Previously, best-of called `budget_ranked_agents`, which classified the prompt
and returned only agent kinds. It checked disabled state but did not enforce the
advice inventory, eligibility or model-group holds. The same-agent child retained
the parent's explicit model, even when advice selected a different model.

Best-of now calls `cmd::advise::build_report` with the declared profile, kind,
team, store and detected caller, with `top=0`. It filters the full ranked builtin
candidate list using `AdviceCandidate::launchable`, keeps the top N, and cycles
full candidates when necessary. It never uses `recommended` as launch authority
or mixes custom advice's separate scale into builtin ranking.

Every child records all four dimensions used by advice. Its model is the advised
model with `AidResolved` provenance, including the same-agent child. Known CLI
defaults are pinned; unknown defaults clear the parent model and use the existing
forced-default override to suppress later budget routing. Model-source rendering
calls these materialized pins `aid-selected model` and uses `forced default`
instead of attributing every default override to a self-heal retry. Served-model
validation rejects a selected model that would otherwise be dropped for a default.

Route-owned session clearing still uses `switch_agent`. Candidate IDs, output and
result paths, metric/delivery selection, and winner finalization remain on their
existing paths. The old ranker and its unused model-tier/candidate helpers were
deleted; historical score inputs remain test fixtures only. The existing best-of
flow was split into launch, collection and announcement functions to keep edited
functions under 50 lines, without a new routing abstraction.

## Controlled regression

`tests/bestof_advise_e2e.rs` installs a synthetic Codex executable in a temporary
PATH and disables other builtin routes in its temporary AID home. It compares
advice with the race, captures actual CLI argv separately for each output path,
and checks the database's requested model, saved provenance/profile, candidate
IDs and final winner artifact. A parent `--model parent-model` deliberately
disagrees with the sticky or CLI-config advice model. Additional cases cover an
unknown default, an all-held fleet with no task rows, and an unserved selected
model with no CLI launch. A real detected-caller environment with a known stronger
model also proves the weaker caller-pool candidate creates no task rows. These
are controlled launches, not provider calls.

The focused plan regressions use real advice with isolated inventory, quota and
auth evidence. They cover unavailable/disabled/auth-failed/below-floor routes,
agent and selected-group holds (including background urgency), a different group
remaining usable, Gemini supersession, Claude preference, weaker caller-pool
models, an advisory fallback that cannot launch, ranked truncation/cycling,
same-agent model disagreement, cross-agent sessions, free/cheap pins, CLI and
unknown defaults under budget pressure, and inferred/declared profile persistence.
Existing metric, ID and artifact regressions are retained.

## Commands and evidence

All Rust compilation and tests use `aid build` / `aid test` through the existing
Cargo PATH shim to `rbox`, on the ensured build box. The warm target
environment is unchanged; rbox serializes builds with its box lock.

- `rbox ensure "$AID_BUILD_BOX"`: `already ensured (no-op)` with network access.
- `rbox status --json`: remote box reachable; shared workload present, so the box
  lock is used. The restricted sandbox alone could not read Tailscale JSON.
- Baseline `aid build check -p ai-dispatch`: succeeded, 0 errors / 0 warnings,
  52.4 seconds.
- First implementation check: failed with 3 unresolved imports (`classifier` in
  advice/fallback, `detect_agents` in fallback), 274.0 seconds. Removing parent imports also
  removed sibling-module access; the necessary imports were restored.
- First `aid test --bin aid run_bestof`: compilation failed with those same
  imports, 206.2 seconds; zero matched tests is not a test pass.
- Corrected `aid build check -p ai-dispatch`: succeeded, 0 errors / 0 warnings,
  23.9 seconds.
- First compiled `aid test --bin aid run_bestof`: 27 tests ran, 25 passed,
  2 failed, 0 ignored, 59.7 seconds. The failing fixtures treated Droid as
  agent-metered by writing `rate-limit-droid`; captured candidate evidence showed
  the selected route remained quota-unknown. Droid reads its selected model's
  `standard` group. The fixtures now use Codex for an agent-wide hold and
  `droid--standard` for the Droid group hold; the all-held fixture also uses that
  group. No production routing change was made to accommodate the fixtures.
- Corrected `aid test --bin aid run_bestof`: `passed: 28 passed, 0 failed,
  0 ignored`, 35.1 seconds; its digest named all 28 executed tests, including
  the nine new plan/model regressions and the existing metric/ID/winner artifacts.
- First `aid test --test bestof_advise_e2e`: 4 ran, 2 passed / 2 failed,
  0 ignored, 58.8 seconds. Both launch-rejection cases passed. Successful races
  reached the SQLite assertion, which incorrectly queried the Rust field name
  `requested_model`; the schema stores it as `model`. The test query was fixed
  to the existing schema column, and a caller-pool CLI regression was added.
- Guide validation with `quick_validate.py default-skills/aid-guide`:
  `Skill is valid!`.

Final `aid test --test bestof_advise_e2e` output:

```text
passed: 5 passed, 0 failed, 0 ignored; command: cargo test --test bestof_advise_e2e; elapsed: 32.5s
ran 5 test(s):
  bestof_caller_pool_filter_excludes_a_weaker_model_before_launch (ok)
  bestof_with_no_launchable_candidates_fails_without_task_rows_or_launches (ok)
  bestof_unserved_selected_model_fails_instead_of_launching_another_default (ok)
  bestof_unknown_default_drops_parent_model_and_materializes_missing_profile (ok)
  bestof_launches_advised_sticky_and_cli_default_models_over_same_agent_explicit_model (ok)
```

Final focused best-of output:

```text
passed: 28 passed, 0 failed, 0 ignored; command: cargo test --bin aid run_bestof; elapsed: 35.1s
ran 28 test(s):
  cmd::run::run_bestof::additional_tests::candidate_artifacts_use_unique_paths_after_first_run (ok)
  cmd::run::run_bestof::additional_tests::suffixed_path_inserts_suffix_before_extension (ok)
  cmd::run::run_bestof::additional_tests::finalize_winner_artifacts_copies_winner_and_cleans_loser (ok)
  cmd::run::run_bestof::additional_tests::evaluate_metric_uses_repo_path_when_worktree_is_absent (ok)
  cmd::run::run_bestof::additional_tests::evaluate_metric_falls_back_to_repo_path_when_worktree_is_stale (ok)
  cmd::run::run_bestof::plan::tests::plan_excludes_superseded_gemini_and_unpreferred_claude (ok)
  cmd::run::run_bestof::plan::tests::plan_keeps_ranked_full_candidates_then_cycles_without_replacing_models (ok)
  cmd::run::run_bestof::tests::best_of_completion_includes_awaiting_input (ok)
  cmd::run::run_bestof::tests::best_of_count_validation (ok)
  cmd::run::run_bestof::plan::tests::plan_filters_unavailable_disabled_auth_failed_and_below_floor (ok)
  cmd::run::run_bestof::plan::tests::plan_excludes_weaker_caller_pool_and_rejects_ineligible_recommendation_fallback (ok)
  cmd::run::run_bestof::plan::tests::plan_excludes_agent_and_selected_model_group_holds_even_in_background (ok)
  cmd::run::run_bestof::plan::tests::advised_plan_materializes_all_default_and_declared_profile_inputs (ok)
  cmd::run::run_bestof::tests::best_of_task_ids_always_use_candidate_suffixes (ok)
  cmd::run::run_bestof::tests::best_of_task_ids_drop_invalid_auto_suffixes (ok)
  cmd::run::run_bestof::tests::pick_best_result_ignores_done_without_successful_outcome (ok)
  cmd::run::run_bestof::tests::pick_best_result_ignores_nan_metric_scores (ok)
  cmd::run::run_bestof::tests::pick_best_result_none_when_no_done (ok)
  cmd::run::run_bestof::tests::pick_best_result_prefers_longest_diff (ok)
  cmd::run::run_bestof::tests::pick_best_result_prefers_metric_score (ok)
  cmd::run::run_bestof::tests::pick_best_result_treats_merged_as_success (ok)
  cmd::run::run_bestof::tests::best_of_task_ids_fall_back_to_random_when_derived_id_is_running (ok)
  cmd::run::run_bestof::tests::best_of_task_ids_ignore_running_base_for_siblings (ok)
  cmd::run::run_bestof::tests::best_of_task_ids_reject_invalid_base_ids_before_reuse (ok)
  cmd::run::run_bestof::plan::tests::racers_replace_same_agent_explicit_model_and_clear_only_cross_agent_sessions (ok)
  cmd::run::run_bestof::plan::tests::racers_pin_known_cli_defaults_and_keep_unknown_defaults_under_budget_pressure (ok)
  cmd::run::run_bestof::tests::best_of_task_ids_truncate_to_fit_task_limit (ok)
  cmd::run::run_bestof::plan::tests::budget_racers_keep_the_advised_pin_and_all_profile_inputs (ok)
```

Additional affected-module validation:

```text
$ aid test --bin aid selection
passed: 77 passed, 0 failed, 0 ignored; command: cargo test --bin aid selection; elapsed: 31.4s
```

The selection digest includes
`agent::selection::selection_score_tests::breakdown_is_bit_identical_to_pre_decomposition_value`
and the quota, auth, supersession, caller-pool and launchability tests. Cascade
regressions in this filter remain unchanged.

```text
$ aid test --bin aid model_info
passed: 12 passed, 0 failed, 0 ignored; command: cargo test --bin aid model_info; elapsed: 116.6s
```

The digest names `materialized_advice_model_is_not_attributed_to_the_user`,
`unpinned_cli_config_default_is_named_with_its_source`,
`model_info_names_final_model_and_precedence_source`, and the existing forced
CLI/adapter/delegate default attribution regressions.

The first `aid test --bin aid official_guide` executed its single consistency
check and failed (101.7 seconds): `--best-of` was now documented but still listed
in `UNDOCUMENTED_FLAGS`. That stale allowlist entry was removed; no coverage
exception was added.

```text
$ aid test --bin aid official_guide
passed: 1 passed, 0 failed, 0 ignored; command: cargo test --bin aid official_guide; elapsed: 33.9s
ran 1 test(s):
  cmd::init::official_guide::tests::official_guide_covers_generated_facts (ok)
```

```text
$ aid test --test aid_guide_e2e
passed: 2 passed, 0 failed, 0 ignored; command: cargo test --test aid_guide_e2e; elapsed: 28.1s
ran 2 test(s):
  official_guide_preserves_safety_invariants (ok)
  official_guide_covers_every_public_command (ok)
```

```text
$ aid test --test init_e2e
passed: 3 passed, 0 failed, 0 ignored; command: cargo test --test init_e2e; elapsed: 32.0s
ran 3 test(s):
  init_creates_default_skills_and_templates (ok)
  init_skips_existing_files_without_overwriting_them (ok)
  init_refreshes_release_managed_official_guide (ok)
```

Plan regressions remain inline in `bestof_plan.rs` (274 lines including tests),
within the requested inline-until-300 convention. Rechecking the final layout:

```text
$ aid test --bin aid run_bestof
passed: 28 passed, 0 failed, 0 ignored; command: cargo test --bin aid run_bestof; elapsed: 31.2s
```

The final digest names the same 28 tests listed above.

Clippy infrastructure result:

```text
$ aid build clippy -p ai-dispatch
failed: 0 errors, 0 warnings; command: cargo clippy -p ai-dispatch; elapsed: 21.4s
$ rbox log "$AID_BUILD_BOX" 4212b5a9a02943a289fd17ae33fd823a --tail 20
error: 'cargo-clippy' is not installed for the toolchain 'stable-x86_64-unknown-linux-gnu'.
help: run `rustup component add clippy` to install it
rbox: job 4212b5a9a02943a289fd17ae33fd823a exited with code 1
```

No clippy lint ran. The missing component is a validation blocker; no toolchain
component or binary was installed and no local Rust compilation was used.

Final compilation:

```text
$ aid build check -p ai-dispatch
succeeded: 0 errors, 0 warnings; command: cargo check -p ai-dispatch; elapsed: 25.4s
```

The final targeted set passed 28 best-of + 5 controlled CLI + 77 selection +
12 model-info + 1 guide-facts + 2 guide E2E + 3 init E2E tests (128 total).

## Production-line delta

`python3 /private/tmp/bestof-production-delta.py` compares each changed production
Rust module with `git show 291aa252:<path>`, excluding test files and each module's
trailing `#[cfg(test)]` section. It counts physical lines, including headers,
imports and blank lines; moved code is counted on both sides.

| Module | Before | After | Delta |
| --- | ---: | ---: | ---: |
| `src/agent/selection.rs` | 96 | 35 | -61 |
| `src/agent/selection_scoring.rs` | 222 | 195 | -27 |
| `src/cmd/run/args.rs` | 273 | 273 | 0 |
| `src/cmd/run/bestof.rs` | 284 | 275 | -9 |
| `src/cmd/run/bestof_plan.rs` | 0 | 60 | +60 |
| `src/cmd/run/dispatch_model_info.rs` | 46 | 48 | +2 |
| **Total** | **921** | **886** | **-35** |

Changed Rust files are at most 300 lines with 2–4 line headers. Edited production
functions are at most 50 lines; no production `unwrap()` or dependency was added.
No workspace formatting was run.

## Limits

This slice does not change cascade selection or pricing. Live provider behavior
is not tested by the controlled CLI. External quota/config changes during the
race remain subject to ordinary dispatch checks. The full workspace suite and
independent cross-review remain delivery gates; neither is claimed here. Clippy
requires the remote toolchain component identified above.
