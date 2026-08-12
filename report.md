Implemented operator-cancellation handling.

Changed:

- Foreground signals and `aid stop` now transition to `TaskStatus::Stopped`.
- Cancellation events/log sentinels are no longer errors/failures.
- Excluded stopped tasks from failure counts and success-rate denominators across CLI, board, TUI, usage, project state, web UI, config history, webhooks, and batch results.
- Genuine crashes and watchdog kills remain `Failed`.
- Added regression tests, including watchdog failure preservation.

Consumers audited and changed: `src/board.rs`, `src/cmd/batch_validate.rs`,
`src/cmd/board_stream.rs`, `src/cmd/config_display.rs`, `src/cmd/summary_cli.rs`,
`src/cmd/stats.rs`, `src/cmd_dispatch.rs`, `src/state.rs`, `src/tui/dashboard.rs`,
`src/tui/stats_legacy.rs`, `src/tui/status_bar.rs`, `src/tui/ui.rs`,
`src/tui/ui_helpers.rs`, `src/usage.rs`, `src/usage_report.rs`,
`src/web/static/app.js`, `src/web/static/style.css`, and `src/webhook.rs`.

Review follow-up: stopped tasks now emit the distinct terminal webhook status
`"stopped"` and use the existing terminal subscription, while the payload keeps
cancellation distinct from failure. The foreground interrupt and `aid stop`
event mappings retain `EventKind::Completion`; comments document that this is
the least-wrong existing terminal kind and is counted by the dashboard tally.

Before fix: focused tests failed with `Failed` instead of `Stopped`, and stopped tasks produced 50% success rates/failure counts.

After fix:

- `aid test --bin aid`: 2,300 passed, 0 failed, 9 ignored
- E2E `e2e_test`: 21 passed, 0 failed
- Watchdog regression: passed
- `aid build`: passed
- Focused follow-up tests: webhook 4 passed; interrupt cleanup 2 passed; stop 1 passed
