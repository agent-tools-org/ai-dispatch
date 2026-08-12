[MILESTONE] Verified fix for the three earlier findings.
[MILESTONE] Analyzed the historical task regression.
[MILESTONE] Spot-checked test fixtures for silent errors.

### 1. Are all three closed?
**PASS**
- **Finding 1 (repo root leak):** Fixed. In `src/cmd/show_output_owned.rs`, `task_output_bases` evaluates `task.effective_dir`, `task.worktree_path`, and `task_dir`. It explicitly omits `task.repo_path`, preventing sibling leaks.
- **Finding 2 (missing base):** Fixed for new tasks. Tracing from `run::dispatch`, tasks go through `prepare_dispatch_with` -> `persist_worktree_setup`. Here, `persistable_effective_dir` securely resolves the runtime directory (from `--dir` or CWD), and `store.update_task_worktree` actively persists it into the `effective_dir` column. Retries (`aid run --retry`) also pass through this flow, inheriting the saved `RunArgs` directory context.
- **Finding 3 (containment):** Fixed. `owned_file_under_base` implements `canonicalize()` and `starts_with()` to enforce strict path containment, correctly mitigating `..` and symlink escapes.

### 2. Historical Tasks
**FAIL**
The migration in `src/store/schema.rs` performs a bare `ALTER TABLE tasks ADD COLUMN effective_dir TEXT;`. Every historical task is assigned `NULL` for `effective_dir`. 
For a pre-migration audit or research task that ran with `--dir` and no worktree, both `effective_dir` and `worktree_path` are now `NULL`. When a user runs `aid show --output`, `owned_output_path` falls back exclusively to the `task_dir`. Since the legacy logic that fell back to the process CWD (`std::env::current_dir()`) was entirely removed, the CLI will **never** find the relative report (e.g., `report.md`), even if the user runs the command from the exact original directory. 
The system will inject: `No task-owned output file for this task (declared: report.md). Falling back to this task's log.` This is a severe silent regression that orphans valid, persisted deliverables for legacy tasks.

### 3. Test Sweep Spot-Check
**PASS**
The mechanical sweep safely assigned `effective_dir: None` across test fixtures without corrupting the test assertions:
- In `judge_diff_tests.rs`, the new `judge_material_reports_missing_owned_output` test intentionally relies on `effective_dir: None` combined with a relative `output_path` ("report.md") to trigger and verify the expected missing-output string.
- In `prompt_context_tests.rs`, tests demanding successful output reading (e.g., `resolve_context_from_wraps_in_fence`) still pass legitimately. They construct their mock output using `NamedTempFile` and store the *absolute* path in `task.output_path`. The new `owned_output_path` logic unconditionally accepts absolute paths that are proven to be files, safely bypassing the `effective_dir` base resolution without masking any failures.

### Verdict
**BLOCK**
The drop of the legacy CWD fallback combined with an un-backfilled schema migration permanently breaks relative report resolution for all pre-existing non-worktree tasks. The migration must extract `dir` from the persisted `RunArgs` JSON payload to backfill `effective_dir`, or `show_output_owned.rs` must implement an explicit, isolated CWD fallback for tasks created prior to the migration.

=== AID TASK t-05dd728e DONE (exit 0) ===
