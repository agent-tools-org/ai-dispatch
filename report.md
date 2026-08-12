## Findings

- High — Fixed `src/store/migrations.rs`: malformed historical `dispatch_args` no longer aborts `Store::open()`, and valid sibling rows are backfilled.
- Historical directory paths are preserved verbatim, matching new-task persistence.

Evidence: regression failed before the fix with `malformed JSON`; now passes.

Verification:

- 53 store tests passed.
- `aid build check` and clippy: 0 errors.
- Commit: `81114335`.