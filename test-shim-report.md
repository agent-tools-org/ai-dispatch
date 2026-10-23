# Test-only entry point classification

KB consulted: `kb test-only shims production entry points` found guidance on derived writes
using the production seam, wiring coverage, and tests that bypass production defaults.

Inventory: 122 entries. Classes: support 58, shim 33, parallel 28, live 3.
Deleted: 46. No tests deleted. Plain `#[test]`: 3608 before / 3608 after.
Named test functions (plain and Tokio): 3841 before / 3841 after.
Every original test name and its assertion-macro count survive (static checks, not execution).

Caller catalogs: `test-shim-callers-1.md`, `test-shim-callers-2.md`, `test-shim-callers-3.md`.

S: test fixture, isolation hook, or fixture-state observation; kept unchanged.
R0: inventory premise REFUTED: method already belongs to production, without `cfg(test)`.
R1: legacy worktree command has no corresponding production command; custody operations differ.
R2: old spawn helper asserts a JSON spawn log; production records Store events and stderr text.
R3: old output helper caps at five messages; production full extraction retains all messages.
R4: historical model auto-learning query has no production caller/equivalent selector.
R5: external dispatch fixture needs private PreparedDispatch; exposing its type exceeds function visibility.
R6: automatic project-label discovery differs from production explicit-project summary formatting.
R7: records synthetic PIDs for kill isolation; production signaling requires real process identities.
R8: legacy path/removal fixtures; production removal enforces custody rather than direct force removal.

R1–R6 and R8 remain unresolved parallel behavior/visibility mismatches, not successful retargeting.
Preserving those assertions while deleting the paths needs a decision on obsolete behavior or scope.
R7 remains for process isolation. No retained item is claimed to prove production reachability.

Choices: retain fixtures; spell defaults at callers; test Store completion through
`record_agent_completion`, then production status settlement where completed_at is asserted.
Agent-scoped quota tests include Claude explicitly because AgentKind::ALL currently omits it.
No production function body changed. Only required visibility and deleted-shim imports changed.
No compile/build/test suite ran here: the brief reserves these for project verification.
Cross-review remains for project verification; no sub-tasks were dispatched.

| # | Item (file key:original line, name) | Class | Action | Callers/reason |
| --- | --- | --- | --- | --- |
| 1 | F01:179 probe_agy_capabilities | support | kept | S |
| 2 | F02:114 DetectAgentsGuard | support | kept | S |
| 3 | F02:119 DetectAgentsGuard impl | support | kept | S |
| 4 | F02:128 DetectAgentsGuard Drop | support | kept | S |
| 5 | F03:69 CursorBinaryGuard | support | kept | S |
| 6 | F03:74 CursorBinaryGuard impl | support | kept | S |
| 7 | F03:82 CursorBinaryGuard Drop | support | kept | S |
| 8 | F04:37 shared_target_dir | shim | deleted | C01 |
| 9 | F05:104 create | shim | deleted | C02 |
| 10 | F05:114 create_from_home | shim | deleted | C03 |
| 11 | F06:34 find_doctor_symlinks | shim | deleted | C04 |
| 12 | F07:117 clear_served_models_cache | support | kept | S |
| 13 | F08:33 new | shim | deleted | C05 |
| 14 | F09:102 record_quota_exhaustion | shim | deleted | C06 |
| 15 | F10:15 set_test_codex_home | support | kept | S |
| 16 | F11:37 for_tests | support | kept | S |
| 17 | F11:86 emitted | support | kept | S |
| 18 | F12:129 get_dir_size_bounded | shim | deleted | C07 |
| 19 | F13:24 stash_local_changes_with_hook | support | kept | S |
| 20 | F13:35 stash_local_changes_with_identity_hook | support | kept | S |
| 21 | F13:46 find_stash_for_test | shim | deleted | C08 |
| 22 | F14:67 prune | parallel | kept/reason | R1 |
| 23 | F14:117 remove | parallel | kept/reason | R1 |
| 24 | F15:82 set_test_hiboss_command | support | kept | S |
| 25 | F16:7 merge_group | parallel | deleted | C09 |
| 26 | F17:7 merge_single | parallel | deleted | C10 |
| 27 | F18:20 run_agent_process | parallel | deleted | C11 |
| 28 | F18:71 spawn_child_with_log | parallel | kept/reason | R2 |
| 29 | F18:85 write_spawn_error_log | parallel | kept/reason | R2 |
| 30 | F18:96 write_streaming_output | parallel | kept/reason | R3 |
| 31 | F19:276 claim_test_dispatch | shim | kept/reason | R5 |
| 32 | F20:141 take_next_cascade_agent | shim | deleted | C12 |
| 33 | F20:144 auto_save_task_output | shim | deleted | C13 |
| 34 | F20:146 read_quota_error_message | shim | deleted | C14 |
| 35 | F20:148 worktree_is_empty_diff | shim | deleted | C15 |
| 36 | F20:150 maybe_run_post_done_audit | shim | deleted | C16 |
| 37 | F20:160 final_dirty_assertion | shim | deleted | C17 |
| 38 | F21:116 worktree_is_empty_diff | shim | deleted | C18 |
| 39 | F22:73 preflight_cli_command_runner | support | kept | S |
| 40 | F23:10 DEFAULT_IDLE_TIMEOUT | support | kept | S |
| 41 | F23:19 idle_timeout_from_tokio_command | shim | deleted | C19 |
| 42 | F24:130 from_command | support | kept | S |
| 43 | F24:135 from_tokio_command | support | kept | S |
| 44 | F24:148 from_env_pairs | support | kept | S |
| 45 | F25:166 list_tools | shim | deleted | C20 |
| 46 | F26:161 set_feed_for_tests | support | kept | S |
| 47 | F26:174 clear_feed_for_tests | support | kept | S |
| 48 | F27:20 set_test_env | support | kept | S |
| 49 | F27:58 mark_exhausted | support | kept | S |
| 50 | F28:211 overrides_marker_at_in_cache | parallel | deleted | C21 |
| 51 | F28:236 group_from_marker_path | parallel | deleted | C22 |
| 52 | F29:146 CacheDirGuard | support | kept | S |
| 53 | F29:151 CacheDirGuard impl | support | kept | S |
| 54 | F29:160 CacheDirGuard Drop | support | kept | S |
| 55 | F30:48 assert_marker_path_isolated | support | kept | S |
| 56 | F31:18 is_rate_limit_error | shim | deleted | C23 |
| 57 | F31:23 is_rate_limit_error_with_evidence | parallel | deleted | C24 |
| 58 | F31:141 test_future_recovery_time | support | kept | S |
| 59 | F31:156 test_ordinal_suffix | support | kept | S |
| 60 | F32:117 mark_rate_limited_for_value | shim | deleted | C25 |
| 61 | F32:268 mark_rate_limited_until | support | kept | S |
| 62 | F33:159 match_quota_signature | shim | deleted | C26 |
| 63 | F33:165 match_quota_signature_with_agent | parallel | deleted | C27 |
| 64 | F34:10 KillTargets | parallel | kept/reason | R7 |
| 65 | F34:77 kill_targets | parallel | kept/reason | R7 |
| 66 | F34:86 KillTargets impl | parallel | kept/reason | R7 |
| 67 | F35:34 id | shim | deleted | C28 |
| 68 | F36:27 run_agent_process | parallel | deleted | C29 |
| 69 | F37:80 new | parallel | deleted | C30 |
| 70 | F38:121 kill | support | kept | S |
| 71 | F39:21 set_test_home | support | kept | S |
| 72 | F40:6 watch_streaming | parallel | deleted | C31 |
| 73 | F41:15 update_task_completion | parallel | deleted | C32 |
| 74 | F41:22 complete_task_atomic | parallel | deleted | C33 |
| 75 | F42:40 DbStats | support | kept | S |
| 76 | F42:83 open_memory | support | kept | S |
| 77 | F42:113 db_stats | support | kept | S |
| 78 | F43:57 load_state_in | support | kept | S |
| 79 | F43:61 load_state_at | support | kept | S |
| 80 | F43:76 save_state_in | support | kept | S |
| 81 | F43:92 refresh_project_state_in | support | kept | S |
| 82 | F43:160 format_state_summary_in | parallel | kept/reason | R6 |
| 83 | F43:242 project_label_in | parallel | kept/reason | R6 |
| 84 | F43:249 project_id_from_aid_dir | parallel | kept/reason | R6 |
| 85 | F44:107 fail_if_running | shim | deleted | C34 |
| 86 | F45:88 replace_waiting_task | shim | deleted | C35 |
| 87 | F46:57 budget_usage_summary_all | shim | deleted | C36 |
| 88 | F47:175 latest_default_model | parallel | kept/reason | R4 |
| 89 | F48:151 AidHomeGuard | support | kept | S |
| 90 | F48:156 AidHomeGuard impl | support | kept | S |
| 91 | F48:165 AidHomeGuard Drop | support | kept | S |
| 92 | F48:187 assert_aid_home_isolated | support | kept | S |
| 93 | F48:204 resolve_path | support | kept | S |
| 94 | F49:28 check_disk_space | shim | deleted | C37 |
| 95 | F50:17 test_env_lock | support | kept | S |
| 96 | F51:33 mark_stopped | shim | deleted | C38 |
| 97 | F51:77 update_task_completion | parallel | deleted | C39 |
| 98 | F52:56 is_conclusive | live | kept | R0 |
| 99 | F53:80 set_generate_sequence_for_tests | support | kept | S |
| 100 | F54:25 ALL | support | kept | S |
| 101 | F55:16 ACTIVE_EXECUTION_FAILURE_STATUSES | support | kept | S |
| 102 | F56:101 conclusive_model | live | kept | R0 |
| 103 | F57:28 ALL | support | kept | S |
| 104 | F57:66 was_attempted | live | kept | R0 |
| 105 | F58:92 new | parallel | deleted | C40 |
| 106 | F59:24 build_task_tree | shim | deleted | C41 |
| 107 | F59:30 build_task_tree_with_creators | shim | deleted | C42 |
| 108 | F60:147 read_task_output_for_tui | shim | deleted | C43 |
| 109 | F61:30 rescue_dirty_worktree | shim | deleted | C44 |
| 110 | F62:187 set_test_but_available | support | kept | S |
| 111 | F62:192 set_test_project_present | support | kept | S |
| 112 | F62:197 set_test_but_command | support | kept | S |
| 113 | F63:26 check_worktree_lock | shim | deleted | C45 |
| 114 | F63:32 check_worktree_lock_with_store | support | kept | S |
| 115 | F63:64 write_worktree_lock | support | kept | S |
| 116 | F63:84 try_acquire_worktree_lock | shim | deleted | C46 |
| 117 | F63:264 simulate_stale_recovery_race | support | kept | S |
| 118 | F64:40 WorktreeHomeGuard | support | kept | S |
| 119 | F64:45 WorktreeHomeGuard impl | support | kept | S |
| 120 | F64:54 WorktreeHomeGuard Drop | support | kept | S |
| 121 | F64:166 is_safe_worktree_path | parallel | kept/reason | R8 |
| 122 | F64:178 remove_worktree | parallel | kept/reason | R8 |

File keys:

| Key | File |
| --- | --- |
| F01 | src/agent/antigravity.rs |
| F02 | src/agent/binary_route.rs |
| F03 | src/agent/cursor.rs |
| F04 | src/agent/env.rs |
| F05 | src/agent/home_isolation.rs |
| F06 | src/agent/home_isolation_symlinks.rs |
| F07 | src/agent/model_validation.rs |
| F08 | src/agent/opencode_overlay.rs |
| F09 | src/agent/stream_completion.rs |
| F10 | src/agent/codex/cli_config.rs |
| F11 | src/cmd/build_progress.rs |
| F12 | src/cmd/clean_size.rs |
| F13 | src/cmd/merge_stash.rs |
| F14 | src/cmd/worktree.rs |
| F15 | src/cmd/merge/approval.rs |
| F16 | src/cmd/merge/group.rs |
| F17 | src/cmd/merge/single.rs |
| F18 | src/cmd/run/agent.rs |
| F19 | src/cmd/run/dispatch_prepare.rs |
| F20 | src/cmd/run/mod.rs |
| F21 | src/cmd/run/post.rs |
| F22 | src/cmd/run/validate.rs |
| F23 | src/config/idle_timeout.rs |
| F24 | src/config/timeout_policy.rs |
| F25 | src/config/toolbox/mod.rs |
| F26 | src/cost/mod.rs |
| F27 | src/routing/credentials.rs |
| F28 | src/routing/availability/policy.rs |
| F29 | src/routing/quota/live.rs |
| F30 | src/routing/rate_limit/mod.rs |
| F31 | src/routing/rate_limit/detection/mod.rs |
| F32 | src/routing/rate_limit/markers/mod.rs |
| F33 | src/routing/rate_limit/signatures/mod.rs |
| F34 | src/runtime/background/kill.rs |
| F35 | src/runtime/process/guard.rs |
| F36 | src/runtime/pty/runner.rs |
| F37 | src/runtime/pty/watch.rs |
| F38 | src/runtime/pty/bridge/mod.rs |
| F39 | src/runtime/sandbox/mod.rs |
| F40 | src/runtime/watcher/input.rs |
| F41 | src/store/completion_mutations.rs |
| F42 | src/store/mod.rs |
| F43 | src/store/state.rs |
| F44 | src/store/status_mutations.rs |
| F45 | src/store/task_insert_mutations.rs |
| F46 | src/store/queries/task_metrics_queries.rs |
| F47 | src/store/queries/task_queries/mod.rs |
| F48 | src/support/paths.rs |
| F49 | src/support/system_resources.rs |
| F50 | src/task/aic.rs |
| F51 | src/task/lifecycle.rs |
| F52 | src/types/attribution.rs |
| F53 | src/types/mod.rs |
| F54 | src/types/status.rs |
| F55 | src/types/status_sets.rs |
| F56 | src/types/task.rs |
| F57 | src/types/verify_status.rs |
| F58 | src/ui/tui/app/mod.rs |
| F59 | src/ui/tui/tree_data/mod.rs |
| F60 | src/ui/tui/ui/ui_helpers.rs |
| F61 | src/workspace/commit/rescue.rs |
| F62 | src/workspace/gitbutler/mod.rs |
| F63 | src/workspace/worktree/lock/mod.rs |
| F64 | src/workspace/worktree/path/mod.rs |

Verification commands and evidence (local, no remote tested HEAD):

- `python3 /tmp/shim-report.py`: PASS; 3,608 plain tests and 3,841 named tests before/after;
  zero missing names and zero per-test assertion-count deficits. This is a static inventory check.
- `python3 /tmp/check-production.py`: PASS after excluding test items, visibility, and imports;
  production token bodies match. Imports and visibility changes were reviewed separately.
- `python3 /tmp/verify-retarget.py`: PASS; Rust syntax parsing, size, added-line width, diff whitespace.
- `~/.local/bin/security-guard --repo "$PWD" staged`: PASS; zero findings.
- No `aid build`, `aid test`, or Cargo compilation: deferred by the brief to project verification.

No test was deleted, so there is no deleted-test coverage substitution to report.
Changes to production execution, watcher, dispatch, lifecycle, and stats bodies were avoided.

Final diff totals: 156 files changed, 3,447 insertions(+), 1,765 deletions(-).
