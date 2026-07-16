# Single-Task Run Exit Status Fix

## Follow-up Round

- Treats completed tasks with `verify_status = failed` as exit 1 while preserving `TaskStatus::Done`.
- Reuses the latest persisted error event to include verification failure details in the final line.
- Prefixes success, task failure, verification failure, and background output with distinct status tags.

## Files Changed

- `src/main.rs`: inspects the completed single-task run outcome, prints its final summary, and exits non-zero for unsuccessful foreground outcomes.
- `src/cmd_dispatch.rs`: adds typed dispatch/run outcomes, verification-aware exit-code mapping, duration formatting, tagged foreground summaries, and tagged background wording.
- `src/cmd_dispatch/tests.rs`: covers dispatch input resolution, verification-aware exit outcomes, and distinct machine-parseable status tags.
- `src/cmd_dispatch/dispatch_match.rs`: returns the single-task run result while preserving command-completed outcomes for other subcommands.
- `src/cmd_dispatch/run_batch.rs`: returns the `TaskId` produced by the single-task `cmd::run::run` call so the top-level dispatcher can assess it.
- `src/cmd/run_lifecycle.rs`: propagates the final retry or cascade task ID instead of an intermediate attempt ID.
- `src/cmd/run_dispatch_execute.rs`: uses explicit background wording that says the task is still running.

## Exit Code Semantics

- Foreground `aid run`: `Done` with pending, skipped, or passed verification exits 0.
- Foreground `aid run`: `Done` with failed verification exits 1 and includes the latest persisted verification error.
- Foreground `aid run`: every terminal status other than `Done`, including `Failed`, `Stopped`, and `TimedOut`, exits 1.
- Background `aid run --bg`: exits 0 after successful dispatch; the eventual task result is not assessed at detach time.
- `aid run --dry-run`: retains exit code 0.
- Other subcommands retain their existing exit behavior.

## Example Terminal Lines

Foreground success:

```text
[STATUS=DONE] [aid] t-1234 done in 12s (exit 0)
```

Foreground verification failure:

```text
[STATUS=VERIFY_FAILED] [aid] t-1234 completed but verification failed in 12s (exit 1) — Failed during verification: cargo check
```

Foreground failure:

```text
[STATUS=FAILED] [aid] t-1234 failed in 12s (exit 1) — agent exited unsuccessfully
```

Background dispatch:

```text
[STATUS=BG_RUNNING] Task t-1234 started in background and is still running (codex: fix exit reporting)
```

## Verification

- `cargo check -p ai-dispatch`
- `cargo test -p ai-dispatch cmd_dispatch::tests` (8 passed, 0 failed)
