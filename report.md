Implemented and committed as `f2447a3c`.

Changed:

- Foreground signals and `aid stop` now transition to `TaskStatus::Stopped`.
- Cancellation events/log sentinels are no longer errors/failures.
- Excluded stopped tasks from failure counts and success-rate denominators across CLI, board, TUI, usage, project state, web UI, config history, webhooks, and batch results.
- Genuine crashes and watchdog kills remain `Failed`.
- Added regression tests, including watchdog failure preservation.

Before fix: focused tests failed with `Failed` instead of `Stopped`, and stopped tasks produced 50% success rates/failure counts.

After fix:

- `aid test --bin aid`: 2,300 passed, 0 failed, 9 ignored
- E2E `e2e_test`: 21 passed, 0 failed
- Watchdog regression: passed
- `aid build`: passed
- Worktree clean.