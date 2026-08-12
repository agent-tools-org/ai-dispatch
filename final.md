Audit verdict: BLOCK

1. THE ONE THAT MATTERS — FAIL

Evidence: target `src/store/migrations.rs:76-89` safely handles:

- Valid non-object JSON: extracted directory is `NULL`.
- Numeric/array/object `dir`: `NULL`.
- `NULL dispatch_args`: `NULL`.
- Malformed or deeply nested JSON: `json_valid` returns `0`; result is `NULL`.
- Tested 1 MB valid payload: extraction succeeded.

However, `row.get::<_, String>(0)?` still propagates for legal SQLite values such as:

- `id = NULL`
- `id = X'00FF'` (BLOB)

SQLite permits both despite `TEXT PRIMARY KEY`; rusqlite’s `String` conversion rejects them. Therefore one unusual row ID can still make `Store::open()` fail.

2. Guard outcome changes — FAIL

Valid non-text `dir` values previously selected but ultimately became `NULL` after `usable_recorded_dir`, so those outcomes are unchanged.

A valid text payload does change:

```json
{"dir":" /tmp/legacy "}
```

Old code trimmed it and stored `/tmp/legacy`. New code sees the leading space, rejects it as non-absolute, and stores `NULL`.

Trailing whitespace also changes the stored value:

```json
{"dir":"/tmp/legacy "}
```

Old: `/tmp/legacy`  
New: `/tmp/legacy `

3. `trim()` removal and persistence agreement — FAIL

For the same absolute input, they now agree:

```text
/tmp/recorded-dir-with-space 
```

Both `persistable_effective_dir` and the new migration preserve it exactly.

They intentionally differ for relative inputs: new-task persistence resolves `relative` against CWD, while migration returns `NULL` because historical CWD is unavailable.

More importantly, an already-migrated database can retain the old trimmed value. The migration update is restricted to `effective_dir IS NULL`, so an existing `/tmp/legacy` is not rewritten. New tasks can store `/tmp/legacy `, producing two values for the same original directory.

Construction sites checked:

- `Task::effective_dir`: `src/types/task.rs:34`
- New-task initialization: `src/cmd/run_dispatch_prepare.rs:181`
- New-task persistence: `persistable_effective_dir`, `src/cmd/run_dispatch_prepare.rs:283-305`
- Store insert/update paths: `src/store/mutations.rs:38-85`, `137-158`, `309-319`
- Schema creation and migration: `src/store/schema.rs:23`, `245-249`
- Row loading: `src/store/schema.rs:284`
- Historical backfill: `src/store/migrations.rs:71-109`

Verification:

- `aid build`: 0 errors, 2 warnings.
- `aid test`: 2,380 passed, 0 failed, 9 ignored.
- `git diff --check c7ef667d^ c7ef667d`: clean.

The suite does not cover NULL/BLOB IDs, leading whitespace, or mixed trimmed/untrimmed historical values. The target commit was not checked out directly because this review was read-only; the available `droid` tool was also unavailable.