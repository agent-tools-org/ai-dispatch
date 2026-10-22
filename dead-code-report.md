# Dead-code removal report

KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb "dead code removal rust"` returned general Rust and target-isolation guidance; no specific deletion recipe.

All 116 compiler-warning entries are accounted for below. Locations refer to the supplied baseline list.
Each entry can name several fields or methods. Test-only helpers remain available to regression tests of live code.
Name matches for unrelated receivers, config fields, and ordinary prose were distinguished from symbol references.

## Per-item disposition

| # | Baseline location and item(s) | Disposition / evidence |
|---|---|---|
| 1 | `src/agent/cargo_target.rs:61:15` `remove_branch_target_dir` | deleted |
| 2 | `src/agent/classifier.rs:142:18` `LOW_VALUE_TERMS` | deleted |
| 3 | `src/agent/classifier.rs:30:9` `complexity` | deleted |
| 4 | `src/agent/classifier.rs:63:12` `label` | deleted |
| 5 | `src/agent/custom.rs:51:9` `trust_tier` | not-dead; kept-with-allow — Existing agent TOML; guide dispatch.md:130 and tests/common/settlement.rs:165 reference trust_tier. |
| 6 | `src/agent/env.rs:109:15` `remove_branch_target_dir_if_unused` | deleted |
| 7 | `src/agent/env.rs:123:8` `apply_rust_build_cache_env` | deleted |
| 8 | `src/agent/env.rs:170:4` `branch_has_live_worktree` | deleted |
| 9 | `src/agent/env.rs:181:4` `worktree_list_has_branch` | deleted |
| 10 | `src/agent/env.rs:30:7` `BASE_TARGET_DIR_NAME` | deleted |
| 11 | `src/agent/env.rs:37:8` `shared_target_dir` | cfg(test) — agent/tests.rs |
| 12 | `src/agent/home_isolation_symlinks.rs:33:15` `find_doctor_symlinks` | cfg(test) — agent/home_isolation_symlink_tests.rs; cmd/doctor.rs tests |
| 13 | `src/agent/home_isolation.rs:102:12` `create`, `create_from_home` | cfg(test) — agent/env_redirects.rs tests; runtime/background/launch.rs tests |
| 14 | `src/agent/mod.rs:128:8` `needs_pty` | deleted |
| 15 | `src/agent/model_validation.rs:106:15` `clear_served_models_cache` | cfg(test) — cmd/agent_json_models_tests.rs |
| 16 | `src/agent/opencode_overlay.rs:17:9` `display_name` | deleted |
| 17 | `src/agent/opencode_overlay.rs:33:12` `new` | cfg(test) — runtime/watcher/opencode_usage_tests.rs |
| 18 | `src/agent/selection_capabilities.rs:37:15` `custom_command_installed` | deleted |
| 19 | `src/agent/stream_completion.rs:101:15` `record_quota_exhaustion` | cfg(test) — routing/rate_limit/claude/claude_tests.rs |
| 20 | `src/batch/schema.rs:18:9` `description` | kept-with-allow — Batch TOML description schema. |
| 21 | `src/cmd/agent_json_helpers.rs:66:15` `custom_has_endpoint` | deleted |
| 22 | `src/cmd/build_fallback.rs:54:15` `should_retry_with_fallback` | deleted |
| 23 | `src/cmd/clean_size.rs:128:15` `get_dir_size_bounded` | cfg(test) — cmd/clean_tests.rs (SizeTracker entry limits) |
| 24 | `src/cmd/clean.rs:181:4` `worktree_paths` | deleted |
| 25 | `src/cmd/clean.rs:189:4` `collect_aid_home_worktree_paths` | deleted |
| 26 | `src/cmd/clean.rs:20:7` `ACTIVE_WORKTREES_SQL` | deleted |
| 27 | `src/cmd/clean.rs:208:4` `collect_legacy_tmp_worktree_paths` | deleted |
| 28 | `src/cmd/merge.rs:241:4` `merge_group` | cfg(test) — cmd/merge/tests.rs (merge workflows) |
| 29 | `src/cmd/merge.rs:48:4` `merge_single` | cfg(test) — cmd/merge/final_branch_tests.rs |
| 30 | `src/cmd/run/agent.rs:19:21` `run_agent_process` | cfg(test) — cmd/run/capacity_continuation_flow_tests.rs |
| 31 | `src/cmd/run/bestof.rs:35:5` `status` | deleted |
| 32 | `src/cmd/run/dispatch_prepare.rs:30:9` `agent`, `log_path`, `repo_path` | deleted |
| 33 | `src/cmd/run/mod.rs:145:15` `read_quota_error_message` | cfg(test) — runtime/watcher/stderr/stderr_tests.rs |
| 34 | `src/cmd/run/post.rs:115:15` `worktree_is_empty_diff` | cfg(test) — cmd/run/tests_quota.rs (Git diff fixtures) |
| 35 | `src/cmd/worktree.rs:116:8` `remove` | deleted |
| 36 | `src/cmd/worktree.rs:66:8` `prune` | deleted |
| 37 | `src/config/idle_timeout.rs:11:15` `env_with_idle_timeout` | deleted |
| 38 | `src/config/idle_timeout.rs:28:15` `idle_timeout_from_command` | deleted |
| 39 | `src/config/idle_timeout.rs:32:15` `idle_timeout_from_tokio_command` | cfg(test) — config/timeout_policy_tests.rs |
| 40 | `src/config/idle_timeout.rs:8:18` `DEFAULT_IDLE_TIMEOUT` | cfg(test) — runtime/watcher/streaming_tests.rs |
| 41 | `src/config/project/identity/mod.rs:48:8` `resolve_project_id_with_config` | deleted |
| 42 | `src/config/project/identity/mod.rs:76:8` `matches_project_filter` | deleted |
| 43 | `src/config/project/mod.rs:141:9` `default`, `research`, `simple_edit` | kept-with-allow — Project TOML agent defaults schema (all three fields). |
| 44 | `src/config/project/mod.rs:84:9` `agents` | kept-with-allow — Project TOML agents table schema. |
| 45 | `src/config/timeout_policy.rs:128:19` `from_command`, `from_tokio_command`, `from_env_pairs` | cfg(test) — runtime/pty/runner.rs test helper; config/timeout_policy_tests.rs |
| 46 | `src/config/toolbox.rs:144:8` `list_tools` | cfg(test) — config/toolbox.rs parser and scope tests |
| 47 | `src/prompt/claudemd.rs:35:8` `read_aid_section` | deleted |
| 48 | `src/prompt/compaction.rs:7:8` `compact_to_budget` | deleted |
| 49 | `src/prompt/skills.rs:117:8` `resolve_skill_content` | deleted |
| 50 | `src/prompt/templates.rs:51:8` `inject_milestone_prompt` | deleted |
| 51 | `src/routing/availability/policy.rs:209:15` `overrides_marker_at` | deleted |
| 52 | `src/routing/availability/policy.rs:216:15` `overrides_marker_at_in_cache` | cfg(test) — routing/quota/billing_tests.rs |
| 53 | `src/routing/availability/policy.rs:240:4` `group_from_marker_path` | cfg(test) — test-only overrides_marker_at_in_cache |
| 54 | `src/routing/credentials.rs:104:4` `select_round_robin` | deleted |
| 55 | `src/routing/credentials.rs:120:4` `select_least_used` | deleted |
| 56 | `src/routing/credentials.rs:138:4` `key_value_if_ready` | deleted |
| 57 | `src/routing/credentials.rs:32:5` `next_index`, `usage_counts` | deleted |
| 58 | `src/routing/credentials.rs:58:8` `get_credential` | deleted |
| 59 | `src/routing/credentials.rs:74:8` `mark_exhausted` | cfg(test) — cmd/credential.rs tests (hold setup) |
| 60 | `src/routing/credentials.rs:98:4` `select_fill_first` | deleted |
| 61 | `src/routing/model_catalog/resolved.rs:38:9` `updated` | kept-with-allow — Pricing-file JSON metadata. |
| 62 | `src/routing/quota/live.rs:41:15` `overrides_marker` | deleted |
| 63 | `src/routing/rate_limit/detection/mod.rs:20:5` `AgentProse` | cfg(test) — routing/rate_limit/rate_limit_tests.rs |
| 64 | `src/routing/rate_limit/detection/mod.rs:23:8` `is_rate_limit_error` | cfg(test) — agent/stream_completion_tests.rs |
| 65 | `src/routing/rate_limit/detection/mod.rs:27:8` `is_rate_limit_error_with_evidence` | cfg(test) — test-only is_rate_limit_error; refusal regression tests |
| 66 | `src/routing/rate_limit/detection/mod.rs:9:8` `quota_signature_agent` | deleted |
| 67 | `src/routing/rate_limit/markers/mod.rs:116:8` `mark_rate_limited_for_value` | cfg(test) — routing/rate_limit/credibility_tests.rs (marker setup) |
| 68 | `src/routing/rate_limit/signatures/mod.rs:158:15` `match_quota_signature` | cfg(test) — routing/rate_limit/signatures/cursor.rs tests |
| 69 | `src/routing/rate_limit/signatures/mod.rs:163:15` `match_quota_signature_with_agent` | cfg(test) — test-only signature matching wrapper |
| 70 | `src/runtime/codex_app_server/control.rs:48:12` `create` | deleted |
| 71 | `src/runtime/codex_app_server/turn/mod.rs:57:12` `pid` | deleted |
| 72 | `src/runtime/process/guard.rs:33:12` `spawn_detached`, `id` | deleted `spawn_detached`; cfg(test) `id` — process-group Drop cleanup test |
| 73 | `src/runtime/pty/bridge/mod.rs:120:12` `kill` | cfg(test) — runtime/pty/watch_refusal_round2_tests.rs (cleanup) |
| 74 | `src/runtime/pty/runner_control/mod.rs:22:19` `mark_interrupted`, `agent_pid`, `wait_agent_pid` | deleted |
| 75 | `src/runtime/pty/runner.rs:25:8` `run_agent_process` | cfg(test) — cmd/stop/settle_launch_tests.rs |
| 76 | `src/runtime/pty/watch.rs:76:19` `new` | cfg(test) — runtime/pty/watch_write_tests.rs (monitor setup) |
| 77 | `src/runtime/pty/watch/finalize/mod.rs:164:15` `append_stopped_terminal_sentinel` | deleted |
| 78 | `src/runtime/watcher/input.rs:5:14` `watch_streaming` | cfg(test) — runtime/watcher/api_route_tests.rs |
| 79 | `src/store/completion_mutations.rs:14:12` `update_task_completion`, `complete_task_atomic` | cfg(test) — cmd/stop/settle_writer_tests.rs (both methods) |
| 80 | `src/store/queries/task_similarity_queries.rs:10:7` `SIMILAR_TASK_STOPWORDS` | deleted |
| 81 | `src/store/queries/task_similarity_queries.rs:15:4` `extract_similar_keywords` | deleted |
| 82 | `src/store/queries/task_similarity_queries.rs:36:12` `find_similar_tasks` | deleted |
| 83 | `src/store/queries/worktree_task_queries.rs:13:12` `has_active_worktree_siblings` | deleted |
| 84 | `src/store/state.rs:167:8` `format_state_summary` | deleted |
| 85 | `src/store/state.rs:171:15` `format_state_summary_in` | cfg(test) — store/state_tests.rs (project state rendering) |
| 86 | `src/store/state.rs:249:4` `project_label_in` | cfg(test) — test-only format_state_summary_in |
| 87 | `src/store/state.rs:255:4` `project_id_from_aid_dir` | cfg(test) — test-only project_label_in |
| 88 | `src/store/state.rs:55:8` `load_state` | deleted |
| 89 | `src/store/state.rs:58:15` `load_state_in` | cfg(test) — store/state_tests.rs (state persistence) |
| 90 | `src/store/state.rs:61:4` `load_state_at` | cfg(test) — test-only load_state_in |
| 91 | `src/store/state.rs:77:15` `save_state_in` | cfg(test) — store/state_tests.rs (state fixtures) |
| 92 | `src/store/state.rs:92:15` `refresh_project_state_in` | cfg(test) — store/state_tests.rs (refresh persistence) |
| 93 | `src/store/status_mutations.rs:106:12` `fail_if_running` | cfg(test) — cmd/stop/settle_writer_tests.rs |
| 94 | `src/store/task_insert_mutations.rs:87:12` `replace_waiting_task` | cfg(test) — cmd/stop/settle_dispatch_tests.rs |
| 95 | `src/task/lifecycle.rs:32:15` `mark_stopped` | cfg(test) — cmd/run/stop_followup_tests.rs (parent setup) |
| 96 | `src/task/lifecycle.rs:59:15` `fail_if_running` | deleted |
| 97 | `src/task/lifecycle.rs:81:15` `update_task_completion` | cfg(test) — cmd/stop/settle_done_tests.rs (completion setup) |
| 98 | `src/types/attribution.rs:55:12` `is_conclusive` | cfg(test) — test-only Task::conclusive_model |
| 99 | `src/types/route.rs:112:4` `model_family` | deleted |
| 100 | `src/types/route.rs:58:12` `metering`, `shares_pool_with` | deleted |
| 101 | `src/types/status_sets.rs:15:11` `ACTIVE_EXECUTION_FAILURE_STATUSES` | cfg(test) — store/execution_mutations.rs tests |
| 102 | `src/types/status.rs:24:15` `ALL` | cfg(test) — types/outcome_tests.rs (exhaustive status matrix) |
| 103 | `src/types/task_profile.rs:176:12` `label`, `parse_str` | deleted |
| 104 | `src/types/task.rs:100:12` `conclusive_model` | cfg(test) — store/queries/task_queries.rs test-only latest_default_model |
| 105 | `src/types/verify_status.rs:27:15` `ALL`, `was_attempted` | cfg(test) — types/outcome_tests.rs; cmd/run/verify_gate_tests.rs |
| 106 | `src/ui/tui/ui/ui_helpers.rs:288:8` `pending_prompt` | deleted |
| 107 | `src/workspace/commit/mod.rs:21:15` `has_changes_since` | deleted |
| 108 | `src/workspace/commit/mod.rs:31:15` `task_has_changes_since` | deleted |
| 109 | `src/workspace/commit/rescue.rs:29:8` `rescue_dirty_worktree` | cfg(test) — workspace/commit/rescue_tests.rs (baseline rescue workflows) |
| 110 | `src/workspace/worktree/live_state/mod.rs:59:15` `worktree_has_uncommitted_changes` | deleted |
| 111 | `src/workspace/worktree/lock/mod.rs:24:8` `check_worktree_lock` | cfg(test) — cmd/stop/final_state_tests.rs |
| 112 | `src/workspace/worktree/lock/mod.rs:29:8` `check_worktree_lock_with_store` | cfg(test) — workspace/worktree/lock_tests.rs |
| 113 | `src/workspace/worktree/lock/mod.rs:60:8` `write_worktree_lock` | cfg(test) — workspace/worktree/lock_tests.rs (stale lock setup) |
| 114 | `src/workspace/worktree/lock/mod.rs:79:8` `try_acquire_worktree_lock` | cfg(test) — cmd/stop/final_state_tests.rs (lock setup) |
| 115 | `src/workspace/worktree/path/mod.rs:165:8` `is_safe_worktree_path` | cfg(test) — workspace/worktree/path/mod.rs test-only remove_worktree |
| 116 | `src/workspace/worktree/validation/mod.rs:25:15` `is_main_working_tree_path` | deleted |

## Deleted tests

33 tests dedicated to deleted items were removed. Remaining tests keep their names and behavior assertions;
assertions that read removed fields now use retained task data where applicable.

- `src/agent/classifier.rs::high_complexity_long` — `TaskProfile::complexity`.
- `src/agent/classifier.rs::low_complexity_short` — `TaskProfile::complexity`.
- `src/agent/env_tests.rs::apply_rust_build_cache_env_sets_target_only` — `apply_rust_build_cache_env`.
- `src/agent/kilo.rs::kilo_reports_kind_and_needs_pty` — `Agent::needs_pty`.
- `src/agent/mimocode/tests.rs::mimocode_needs_pty` — `Agent::needs_pty`.
- `src/agent/opencode_tests.rs::codex_does_not_need_pty` — `Agent::needs_pty`.
- `src/agent/opencode_tests.rs::opencode_needs_pty` — `Agent::needs_pty`.
- `src/agent/selection_leaderboard_tests.rs::leaderboard_complexity_has_no_self_assigned_bonus` — `TaskProfile::complexity`.
- `src/cmd/build_fallback_tests.rs::retry_skipped_when_cargo_succeeded` — `should_retry_with_fallback`.
- `src/cmd/build_fallback_tests.rs::retry_skipped_when_no_target_dir` — `should_retry_with_fallback`.
- `src/cmd/clean_tests.rs::legacy_tmp_worktree_path_is_collectable` — `collect_legacy_tmp_worktree_paths`.
- `src/cmd/clean_tests.rs::non_aid_tmp_path_is_rejected_by_clean_scan` — `collect_legacy_tmp_worktree_paths`.
- `src/config/idle_timeout.rs::default_idle_timeout_is_600_seconds` — `idle_timeout_from_command`.
- `src/prompt/claudemd.rs::read_returns_none_without_file` — `read_aid_section`.
- `src/prompt/claudemd.rs::read_returns_section` — `read_aid_section`.
- `src/prompt/compaction.rs::compact_to_budget_leaves_short_text` — `compact_to_budget`.
- `src/prompt/compaction.rs::compact_to_budget_truncates_and_marks` — `compact_to_budget`.
- `src/routing/credentials.rs::fill_first_returns_first_non_exhausted_key` — `get_credential and its rotation selectors`.
- `src/routing/credentials.rs::mark_exhausted_skips_exhausted_keys` — `get_credential and its rotation selectors`.
- `src/routing/credentials.rs::round_robin_returns_keys_in_order` — `get_credential and its rotation selectors`.
- `src/runtime/process/guard.rs::spawn_detached` — `ProcessGuard::spawn_detached`.
- `src/store/queries/tests.rs::active_worktree_siblings_counts_stalled_tasks` — `has_active_worktree_siblings`.
- `src/store/queries/tests.rs::finds_matching_tasks_with_keyword_scores` — `find_similar_tasks`.
- `src/store/queries/tests.rs::includes_merged_tasks_in_results` — `find_similar_tasks`.
- `src/store/queries/tests.rs::limits_results_to_positive_scores` — `find_similar_tasks`.
- `src/types/route_tests.rs::a_subscription_shares_across_models` — `Route::metering / shares_pool_with`.
- `src/types/route_tests.rs::agy_families_do_not_share_a_pool` — `Route::metering / shares_pool_with`.
- `src/types/route_tests.rs::api_account_routes_have_independent_spend_pools` — `Route::metering / shares_pool_with`.
- `src/types/route_tests.rs::codex_models_share_one_account_pool` — `Route::metering / shares_pool_with`.
- `src/types/route_tests.rs::different_clis_do_not_share_a_pool` — `Route::metering / shares_pool_with`.
- `src/types/route_tests.rs::redirecting_the_provider_drops_the_default_metering` — `Route::metering / shares_pool_with`.
- `src/types/route_tests.rs::unknown_providers_never_share` — `Route::metering / shares_pool_with`.
- `src/workspace/commit/mod.rs::has_changes_since_detects_commit_and_dirty_tree` — `has_changes_since`.

## Cascades and choices

- Removed `Complexity`, its computation and `complexity_for` after removing the unread field and all writes.
- Historical score fixtures pass their former cheap/premium tier directly; expected scores remain unchanged.
- Removed `ClaimedDispatch::log_path` after deleting the duplicate field on `PreparedDispatch`.
- Gated `Store::latest_default_model` with cfg(test), matching its callers and conclusive-model helpers.
- Deleted three empty modules and their declarations/re-exports: compaction, task similarity, worktree sibling queries.
- Preserved serialized fields with seven field-level allows; no crate/module-level dead-code allowance was added.
- Common-word matches such as `label`, `pid`, `agent_pid`, and `metering` on other types remain untouched.

## Verification

- `rg -n -w <item> src tests default-skills/aid-guide`: call-site and cascade review for listed symbols.
- `python3 /tmp/check-dead-code.py`: PASS, 649 static assertions; 96 Rust files parsed; 116-entry accounting,
  added-line limits, preserved clippy allowances, and removed-test inventory; no Rust compilation.
- Syntax parser command per touched file: `rustfmt --edition 2024 --emit stdout --config skip_children=true <file>`.
  Output was discarded; no formatter changes were applied.
- `git diff --check`: whitespace check.
- `/Users/ming/.local/bin/security-guard repo "$PWD"`: tracked worktree credential scan.
- Compile/test counts: 0 run. No remote tested HEAD. Default/web warning counts and test-build type checking are unverified.
- Cross-review has not run. Existing oversized files and unrelated code were left unchanged.

## Diff totals

Cleanup: 100 files changed, 335 insertions(+), 1214 deletions(-). Combined with allowance removals: 101 files changed, 335 insertions(+), 1217 deletions(-).
