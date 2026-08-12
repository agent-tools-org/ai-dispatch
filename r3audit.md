1. **PASS — migration is wired and idempotent, but not once-only.**

Evidence: `Store::open()` calls `create_tables()` → `migrate()` → `migrate_effective_dir()` ([mod.rs:62-71](</Users/mingsun/.aid/worktrees/ai-dispatch-6b31c929/fix/show-output-task-scope/src/store/mod.rs:62>), [schema.rs:245-250](</Users/mingsun/.aid/worktrees/ai-dispatch-6b31c929/fix/show-output-task-scope/src/store/schema.rs:245>)).

It runs on every open. Existing populated rows are protected by `WHERE effective_dir IS NULL`; retries cannot overwrite them. Partial progress is safe: completed rows remain populated and later runs process only NULL rows. However, there is no migration marker or transaction, so the scan repeats and a failure can leave partial progress.

2. **FAIL — malformed JSON can prevent the database from opening.**

| Payload | Result |
|---|---|
| Malformed/empty JSON | Migration fails with `malformed JSON`; `Store::open()` fails. Reproduced with SQLite. |
| Missing `dispatch_args` | `effective_dir` remains NULL. Safe. |
| Missing `dir` | NULL. Safe. |
| `"dir": null` | NULL. Safe. |
| Relative dir | NULL. Safe; cannot reintroduce CWD resolution. |
| Absolute dir with trailing slash | Stored unchanged and resolves correctly. |
| Absolute dir no longer existing | Absolute value is retained; output lookup reports absence rather than using CWD. |

No valid case above silently redirects one task to another task’s file. The malformed case is worse: it can brick every subsequent normal database open.

The extraction is at [migrations.rs:76-105](</Users/mingsun/.aid/worktrees/ai-dispatch-6b31c929/fix/show-output-task-scope/src/store/migrations.rs:76>).

3. **FAIL — coverage gap in the test’s claimed entry point.**

The test does not call `migrate_effective_dir` directly and would fail if the backfill were removed. However, it constructs `Store` manually and calls `store.migrate()` directly ([effective_dir_migration_tests.rs:103](</Users/mingsun/.aid/worktrees/ai-dispatch-6b31c929/fix/show-output-task-scope/src/store/tests/effective_dir_migration_tests.rs:103>)); it does not exercise `Store::open()`/`create_tables()`. Thus it proves the schema migration path, but not the real open-time wiring.

Verification evidence:

- Focused test: 1 passed.
- Full binary suite: 2335 passed, 0 failed, 9 ignored.
- Guide integration suite: 10 passed.

What I missed: the migration has no `json_valid()` guard, so one corrupt persisted payload makes the startup migration fail permanently until the row is repaired. Also, `trim()` changes legitimate paths with leading/trailing whitespace, and the test does not cover malformed JSON, re-opening an already-migrated store, populated rows, or a missing absolute directory.

**Overall verdict: BLOCK.**