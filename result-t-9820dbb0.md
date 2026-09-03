## Findings

No findings.

## Result

- Added `result_file_required` to `BackgroundRunSpec` with a serde default for older job files, and propagated it from dispatch into worker post-run lifecycle arguments.
- Added `judge_retry` to the same boundary; judge-generated retries now retain their loop guard when executed by a worker.
- Added a real `--bg` Codex delivery E2E case. It waits for the worker and verifies `failed / missing_final_delivery`.

## RunArgs / BackgroundRunSpec audit

- `agent_name` — crosses directly.
- `prompt` — crosses as the resolved prompt.
- `prompt_file` — does not cross; dispatch consumes it into `prompt` before persistence.
- `repo` — does not cross; dispatch-only repository selection is persisted as task/worktree paths.
- `repo_root` — does not cross; dispatch-only repository-root selection is consumed during setup.
- `dir` — crosses directly.
- `output` — crosses directly.
- `result_file` — crosses directly.
- `result_file_required` — crosses directly; this is the fix in this change.
- `model` — crosses as the effective model selected during dispatch.
- `model_source` — does not cross into the spec; it is persisted in dispatch args and loaded separately by lifecycle reconstruction.
- `declared_difficulty` — does not cross; it is persisted in the task profile declaration.
- `declared_budget` — does not cross; it is persisted in the task profile declaration, while the resolved `budget` flag crosses.
- `declared_urgency` — does not cross; it is persisted in the task profile declaration.
- `declared_rigor` — does not cross; it is persisted in the task profile declaration.
- `declared_egress` — does not cross; its dispatch preflight decision is complete before the worker starts.
- `kind` — does not cross; category resolution is persisted on the task and prompt/toolbox injection is already in the resolved prompt.
- `worktree` — crosses directly.
- `base_branch` — crosses directly.
- `group` — crosses directly.
- `verify` — crosses directly.
- `setup` — crosses directly.
- `iterate` — crosses directly.
- `eval` — crosses directly.
- `eval_feedback_template` — crosses directly.
- `judge` — crosses directly.
- `peer_review` — crosses directly.
- `max_duration_mins` — crosses as the normalized duration policy.
- `max_task_cost` — crosses directly.
- `retry` — crosses directly.
- `context` — does not cross; context is expanded into the resolved prompt before the worker starts.
- `checklist` — crosses directly.
- `skills` — crosses directly.
- `hooks` — crosses directly.
- `template` — crosses directly.
- `background` — does not cross; the worker lifecycle intentionally reconstructs `background: true`.
- `dry_run` — does not cross; dry-run exits before worker dispatch.
- `announce` — does not cross; it controls dispatch-terminal output, not worker behavior.
- `foreground` — crosses directly to select foreground lifecycle rendering.
- `parent_task_id` — crosses directly.
- `on_done` — crosses directly.
- `cascade` — crosses directly.
- `read_only` — crosses directly.
- `audit_report_mode` — crosses directly.
- `sandbox` — crosses directly.
- `container` — crosses directly.
- `budget` — crosses directly as the resolved budget mode.
- `best_of` — does not cross; best-of is the outer orchestrator and each candidate clears this field before worker dispatch.
- `metric` — does not cross; best-of consumes it while selecting the winning candidate.
- `session_id` — crosses directly.
- `team` — does not cross; team-derived toolbox instructions are already in the resolved prompt.
- `context_from` — does not cross; referenced task output is expanded into the resolved prompt.
- `batch_siblings` — does not cross; sibling summaries are expanded into the resolved prompt.
- `scope` — crosses directly.
- `env` — crosses directly.
- `env_forward` — crosses directly.
- `judge_retry` — crosses directly; this change closes the judge-retry worker gap.
- `existing_task_id` — does not cross; it is consumed while claiming the task and cleared before normal worker dispatch.
- `timeout` — does not cross directly; it is normalized into `timeout_policy`, environment values, and spec duration fields.
- `idle_timeout_secs` — crosses as the normalized idle policy value.
- `timeout_policy` — does not cross as a Rust value; its effective values are persisted through environment and spec duration fields.
- `audit` — crosses directly.
- `audit_explicit` — crosses directly.
- `no_audit` — crosses directly.
- `suppress_nested_repo_warning` — does not cross; it only suppresses a dispatch-time warning.
- `link_deps` — crosses directly.
- `force_default_model` — does not cross; it is consumed during dispatch model resolution before the worker spec is created.

## Verification

- `aid build`: passed.
- `aid test --test codex_delivery_e2e`: 7 passed.
- `aid test --bin aid background_spec`: 6 passed.
- `aid test --bin aid background_reaper`: 6 passed.
- `scripts/release.sh --dry-run 10.40.0`: the literal invocation is unsupported by the current script because it requires `<version> <notes-file>`; with its required notes file and inherited AID orchestration variables cleared, the clean-worktree dry run passed all tests and release checks.
