## Findings

- High: Auto-assigned audit result files no longer collide in shared `--dir` runs. `src/cmd/run_dispatch_prepare.rs` now rewrites the default audit result file to `result-<task_id>.md` after the final task ID is known, so parallel tasks stop overwriting each other's prompt-directed `result.md`.
- Medium: Audit report mode now skips auto-injecting a result file when `output` is already set. `src/cmd/report_mode.rs` only assigns the default result file when no explicit output destination exists, which removes the redundant `result.md` write path for tasks that already have an `output` file.
- Info: `aid show --result` did not need code changes. It already reads the per-task persisted copy under the task directory, and existing coverage still passes (`result_text_reads_task_result_file` and `read_task_output_uses_persisted_result_file`).
