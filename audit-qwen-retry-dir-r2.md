I'll start by reading the new commit and tracing the retry-dir and stderr-gate changes against the original findings.HEAD is on `main`; I'll inspect `a4cb609e` in place without checking it out.The rework is in view. Next I’ll trace the retry path, `effective_dir`, and the watcher exit-status gate against the original findings.I have the retry and watcher code. Next I’ll trace how `effective_dir` is recorded for `--repo` and `--worktree`, and whether other replay consumers still see a mutated `dir`.`retry.rs` may be over the 300-line limit. I’ll confirm that and trace the remaining replay and watcher paths.[MILESTONE] Read a4cb609e and confirmed the prepare-side cwd mutation is gone
[MILESTONE] Traced resolve_replay_dir against --repo, --worktree, and the original qwen case
[MILESTONE] Traced the watcher stderr gate against idle-timeout, cost-kill, UTF-8, and capture paths

# Re-audit: `a4cb609e` retry dir + stderr cap

Scope: verify the accepted rework of findings 1–5 on `fix/qwen-retry-dir-capture` (`a4cb609e`). Previous audit target was `f07b59ac`. Read-only; suite not re-run (stated 2359/0).

---

## Q1 — Findings 1–4 closed? Is retry-side resolution correct?

### Finding 1 — persist mutation changed `dispatch_args.dir` for every replay consumer

**PASS** — `src/cmd/run_dispatch_prepare.rs:153-155`

`git diff main a4cb609e -- src/cmd/run_dispatch_prepare.rs` is empty. Persist now clones `args` and writes `model` only. Construction site of persisted `dir` is the live flag again, not process cwd.

Other persist consumer (`update_task_dispatch_args`) is this one site. Batch retry / `--retry N` therefore see the original `null` / `.` / explicit path, not a rewritten invocation cwd.

### Finding 2 — `--repo` retries

**PASS** — `src/cmd/retry.rs:151-152,168-187`, `src/cmd/run_prompt_helpers.rs:239-240`

`--repo` with no `--dir` now replays **in the repo directory**.

First run: `resolve_worktree_paths` sets `effective_dir = resolve_dir_in_target(repo, None, repo)` = the repo (`run_prompt_helpers.rs:239-240`). `persistable_effective_dir` stores that absolute path (`run_dispatch_prepare.rs:283,293-301`). Saved `dispatch_args.dir` stays `null`.

Retry: `resolve_retry_target` returns `(None, None)` (`retry.rs:254`). `finish_retry_run_args` calls `resolve_replay_dir`. Saved dir is `null` → process-cwd meaning → `task.effective_dir` if it is a live directory → that is the repo. Test: `retry_repo_without_dir_replays_repo_not_process_cwd`.

### Finding 3 — `--worktree` retries

**PASS** — `src/cmd/retry.rs:149-152,235-254`

Untouched.

- Live worktree: `resolve_retry_target` returns `(Some(worktree_path), Some(branch))`. The `if let Some(dir)` branch sets `dir` and never calls `resolve_replay_dir`.
- Pruned worktree: returns `(None, Some(branch))`. `worktree_arg.is_none()` is false, so `resolve_replay_dir` is skipped; `run` recreates the worktree from the branch.

### Finding 4 — use `task.effective_dir`

**PASS** — `src/cmd/retry.rs:177-181`

First fallback is `task.effective_dir` when `Path::is_dir`. That is the right recorded answer.

### Original qwen case (no `--dir`, no `--repo`, no `--worktree`)

**PASS** — same absolute directory as the first run.

Qwen is not in the auto-set `--dir .` list (`run_dispatch_resolve.rs:119-137`), so the first run stores `dir: null`. `resolve_worktree_paths` returns `args.dir.clone()` = `None` (`run_prompt_helpers.rs:242`). The agent inherits process cwd (`qwen.rs:52-54`). `persistable_effective_dir(None)` records `current_dir().join(".")` (`run_dispatch_prepare.rs:295-301`) — an absolute form of that cwd.

Retry: saved dir is `null` → `effective_dir` if still a directory → `run_args.dir` is that absolute path. Test: `retry_without_dir_replays_absolute_effective_dir`.

`Command::current_dir` on `$CWD/.` plus POSIX `getcwd` yields the same directory the first run inherited.

### Fallback chain (`resolve_replay_dir`)

Only when retry CLI `--dir` is absent **and** `worktree_arg` is `None`:

| Step | Condition | Result |
|------|-----------|--------|
| 0 | Saved `dir` is a real path (not `null` / empty / `.` after trim) | Leave it. No existence check. |
| 1 | `task.effective_dir` is a live directory | Use it. |
| 2 | Else `task.repo_path` is a live directory | **Silent substitution** to the repo. Test: `retry_missing_recorded_dir_falls_back_to_repo`. |
| 3 | Else | **Refuse.** Does not fall back to process cwd. Test: `retry_refuses_when_recorded_dir_and_repo_are_unusable`. `retry.rs:189-192`. |

A recorded `effective_dir` that no longer exists is **not** refused immediately: it is replaced by `repo_path` when that directory still exists, and refused only when both are unusable.

An **explicit** saved `--dir /gone/path` (not `.`) is **not** in this chain. It is replayed as-is (`retry.rs:174-175`). Later spawn/prepare may fail. Different case from the process-cwd meaning.

---

## Q2 — Finding 5 (stderr)

### Gate excludes idle-timeout and cost-kill?

**PASS** for the named Unix cases.

Gate (`watcher.rs:216-218`): `Failed` AND `full_output` empty AND `exit_status.code().is_some()`.

- Idle-timeout (`watcher.rs:85-97`): `force_kill_process_group` then `child.kill()`. On Unix a signal death makes `ExitStatus::code()` `None`. Dump skipped. Test: `streaming_watch_signal_killed_process_does_not_replay_stderr`.
- Cost-kill (`watcher.rs:130-146`): same kill path. Also, cost is observed from a stdout event, so `full_output` is almost never empty. Double exclusion.

Comment says “non-zero exit code”; the check is `code().is_some()` (includes `Some(0)` if some other path already marked `Failed`, e.g. quota). That does not re-open idle-timeout/cost-kill on Unix.

Residual, not a reopen: `force_kill_process_group` starts with SIGTERM and a 3s grace (`process_group.rs:36-44`). A process that converts SIGTERM into `exit(N)` with empty stdout can still enter the dump path. The 64 KiB cap still applies. Windows `code()` is typically `Some(_)` even after kill — not verified here.

### Cap cannot produce invalid UTF-8?

**PASS** — `src/watcher.rs:285-296`

Read is `take(64KiB+1)`, then `truncate(64KiB)`, then `String::from_utf8_lossy`. A mid-sequence cut becomes U+FFFD. The `String` is always valid UTF-8. Marker `\n[stderr truncated]` is ASCII.

No dedicated mid-UTF-8 unit test; this is by construction. Integration cap test uses ASCII `yes x`.

### Watcher regressions?

**PASS** for the changed path.

- Fast-fail qwen still copies stderr: `exit 1`, empty stdout, `code() = Some(1)`. Existing test `streaming_watch_fast_fail_preserves_stderr_in_log` unchanged.
- Loop, completion merge, quota, Codex delivery guard, event insert, `failure_stderr_note` (path note + quota scan) unchanged.
- Buffered watcher never had this dump; qwen is streaming.
- Capture file (`watcher/stderr.rs`) is still unbounded in memory/on disk. Only the **log copy** is capped. That matches finding 5.

---

## Construction sites of persisted `dir`

| Site | Updated? |
|------|----------|
| `prepare_dispatch` persist (`run_dispatch_prepare.rs:153-155`) | Yes — mutation removed. |
| Auto-set `--dir .` for some agents (`run_dispatch_resolve.rs:119-136`) | Pre-existing; qwen not included. Retry now treats `.` as process-cwd and resolves `effective_dir`. Correct. |
| `aid retry` `finish_retry_run_args` | New resolver; gated off worktree / explicit retry `--dir`. |
| `apply_retry_target` (batch / `--retry N`) | Unchanged. Sees original flags again (finding 1 closed). Does not call `resolve_replay_dir`. |

---

## What I missed

- `src/cmd/retry.rs` is 328 lines and `src/watcher.rs` is 336; both now exceed the 300-line cap. `read_capped_stderr` belongs in `src/watcher/stderr.rs`.
- Official guide (`default-skills/aid-guide`, especially `task-operations.md` / `dispatch.md`) was not updated for the new retry invariant: pin to recorded `effective_dir`, refuse rather than guess cwd.
- `aid batch retry` and in-process `--retry N` still do not use `resolve_replay_dir`. After the persist revert they are no longer poisoned; they also do not get the qwen absolute-dir pin unless `apply_retry_target` already has a repo/worktree.
- Explicit stale `--dir` is replayed, not refused. Only `null` / empty / `.` enter the fallback/refuse chain.
- New tests do not assert a `--worktree` retry leaves `dir`/`worktree` unchanged; that rests on the `worktree_arg.is_none()` gate and older tests.
- `failure_stderr_note` still `read_to_string`s the whole stderr file for quota scanning. Not the log-dump path.

## Could not check

- Stated 2359/0 suite: not re-run (instruction).
- Windows kill/`ExitStatus::code()` behavior.
- Live qwen session-key equality for `$CWD` vs `$CWD/.` (POSIX `getcwd` should match; not executed).
- SIGTERM-cooperative agent under idle-timeout with empty stdout (gate leak, still capped).

---

## Verdict

**SHIP**

Findings 1–4 are closed. Retry-side resolution is correct for `--repo`, `--worktree`, and the original qwen case. Finding 5’s dump is gated off Unix signal kills and capped without producing invalid UTF-8. Residuals above are follow-ups, not a reopen of the accepted findings.