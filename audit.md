## Findings

1. **High — the ownership fix still leaks reports through a shared repository root.** `owned_output_path()` accepts any existing `repo_path/<relative -o>` file (`src/cmd/show_output_owned.rs:21-39`). Two worktree tasks using `-o report.md` therefore share the same repo-root report; task B can display task A’s file if B’s worktree lacks one. The regression test only covers foreign CWD files, not foreign repo-root files.

2. **High — legitimate relative outputs under the effective `--dir` no longer resolve.** Without a worktree, `resolve_worktree_paths()` returns `args.dir` but no `repo_path` (`src/cmd/run_prompt_helpers.rs:239-242`); the task stores neither effective directory nor CWD. Thus audit/research tasks using `--dir /external/project -o report.md` fail to find their genuine report. The same occurs for nested `--dir` paths inside a worktree/repository because only the worktree/repo roots are searched. Worktree-root and repo-root outputs do resolve. Pruned worktrees and moved absolute files were already unreadable, so those are not new regressions.

3. **Medium — relative path containment is not enforced.** `base.join(path)` accepts `..`, and `is_file()` follows symlinks (`src/cmd/show_output_owned.rs:21-24`). A declared path such as `../other-task/report.md`, or a symlink beneath a task directory, can escape the intended task boundary.

### Question 1 — FAIL

Evidence is findings 1–2. The new resolver stops finding real reports written in an effective `--dir` that is not persisted as a resolver base. It does not regress:

- worktree-root output;
- repository-root output;
- task-directory output;
- absolute output paths that still exist;
- outputs lost when the worktree itself was pruned;
- absolute paths whose files were moved, which also failed before.

### Question 2 — FAIL

- `src/cmd/judge.rs`: before, consumed the declared path relative to CWD, then `worktree/<path>`; now consumes task-owned output or persisted `result.md` through `read_task_output()`. If unresolved and no diff exists, it degrades to the explicit `(no diff or output)` placeholder.
- `src/cmd/prompt_context.rs`: before, read `task.output_path` directly relative to CWD; now uses the strict resolver and then the same task log fallback. A missing report can silently become log-derived context; with no usable log it warns and skips the task, so the caller’s intended report context may be absent.
- `src/cmd/summary_conclusion.rs`: before, read the declared path relative to CWD; now uses the strict resolver, then the task log. Missing output can silently become a log conclusion or an empty conclusion, rendered as `(none)` by `summary.rs:101-105`.

### Question 3 — PASS, narrowly

For `aid show --output`, `show.rs:109-110` reaches `render_task_output()`. If no declared owned file or persisted `result.md` exists, `show_output_owned.rs:54-56` prefixes:

> No task-owned output file for this task (...). Falling back to this task's log.

The body is then the task’s own log, or `No output or log available` (`show_output_messages.rs:38-53`). This cannot be mistaken for a successful report. The shared-repository collision in Finding 1 can nevertheless produce a wrong successful report before the absence path runs.

## Open Questions

- The added `TempCwd` test helper mutates process-global CWD (`show_result_tests.rs:15-25`); parallel tests can interfere with one another.
- No added test covers no-worktree `--dir`, nested `--dir`, shared repo-root collisions, `..`, symlinks, judge input, context injection, or summary conclusions.
- Targeted validation passed 5/5 `show_result_tests`; this does not cover the failures above.

**Overall verdict: BLOCK**