Implemented and committed:

- Stopped webhooks now emit `"stopped"` and route through the existing terminal subscription, distinct from `"failed"`.
- Documented the `EventKind::Completion` cancellation mapping and dashboard-counting imprecision.
- Updated `report.md` with consumer coverage and regression evidence.

Verification:

- Before: webhook test failed (`None` vs `Some("stopped")`).
- After: webhook 4 passed; interrupt cleanup 2 passed; stop 1 passed.
- `aid build`: 0 errors, 1 warning.
- Existing full suite: 2,300 passed; E2E 21 passed; watchdog regression passed.
- Worktree clean.