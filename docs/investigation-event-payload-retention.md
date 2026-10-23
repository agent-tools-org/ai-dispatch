KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb event metadata retention vacuum`.
The search returned retention and rowid-cursor guidance, including
`infra-ops/a-rowid-cursor-goes-backwards-when-the-table-empties.md`.

# Event payload retention

The supplied investigation was read completely. CONFIRMED: large historical metadata
contains both display-only command output and provider evidence still read by routing.
The supplied database was opened read-only; all measurements mutated a disposable backup.
No live database or task-directory artifacts were modified.

## Key inventory

Value sizes below are compact UTF-8 JSON bytes, including string escaping; MiB = 1,048,576 bytes.
Rows count occurrences of the key. Every event/key pair above 100,000 total bytes is shown;
smaller pairs are not material storage contributors. Max is bytes per value.

| Event type | Key | Rows | Total MiB | Max bytes |
| --- | --- | ---: | ---: | ---: |
| tool_call | provider_stdout | 54,844 | 104.53 | 125,509 |
| error | provider_stdout | 3,577 | 74.36 | 2,197,615 |
| test | full | 1,724 | 68.36 | 3,885,901 |
| tool_call | full | 120,794 | 60.15 | 65,883 |
| build | full | 1,038 | 43.06 | 3,474,679 |
| tool_call | command | 101,701 | 42.51 | 65,883 |
| build | provider_stdout | 270 | 18.58 | 2,169,868 |
| error | full | 2,169 | 18.12 | 3,953,152 |
| reasoning | provider_stdout | 11,207 | 12.91 | 24,424 |
| reasoning | full | 21,057 | 9.04 | 30,874 |
| test | provider_stdout | 390 | 8.48 | 888,222 |
| file_write | provider_stdout | 3,053 | 7.65 | 48,757 |
| milestone | codex_rate_limits | 18,144 | 5.64 | 327 |
| error | command | 13,907 | 5.34 | 17,510 |
| noop | provider_stdout | 216 | 4.31 | 944,332 |
| file_write | files | 20,179 | 3.73 | 6,829 |
| completion | provider_stdout | 1,385 | 3.39 | 13,465 |
| commit | command | 2,665 | 1.23 | 14,761 |
| commit | full | 2,517 | 1.22 | 14,761 |
| tool_call | status | 92,991 | 1.15 | 13 |
| commit | provider_stdout | 888 | 1.01 | 17,001 |
| file_write | full | 9,813 | 0.99 | 180 |
| test | command | 1,732 | 0.78 | 11,985 |
| build | command | 1,042 | 0.43 | 13,156 |
| reasoning | agent_session_id | 10,375 | 0.38 | 38 |
| file_read | full | 2,261 | 0.28 | 253 |
| file_read | files | 2,321 | 0.26 | 239 |
| milestone | model | 18,851 | 0.23 | 14 |
| reasoning | model | 10,348 | 0.17 | 19 |
| milestone | tokens | 18,116 | 0.12 | 8 |
| milestone | input_tokens | 18,116 | 0.12 | 8 |
| milestone | cached_input_tokens | 18,116 | 0.12 | 8 |

## Trim list and reader proof

Eligible tasks must be terminal with completed_at older than the cutoff. A string
payload must contain at least 4,096 UTF-8 bytes; smaller values stay unchanged.

| Task status | Trim keys | Additional conditions |
| --- | --- | --- |
| done, merged | full, provider_stdout | Any event type or command exit code |
| failed, stopped, skipped | full | Prior successful-command rule below |
| Any nonterminal, recent, or missing/invalid completion time | none | Preserved |

The conservative rule applies only to tool_call, build, test, file_read, file_write,
lint, and format events: metadata must have a string command, integer exit_code=0,
status other than in_progress, and a full value different from command. Producer
proof: `src/agent/codex/events/commands.rs:36` copies aggregated_output into full
alongside command and exit_code; the start path records status=in_progress.
On successful tasks the full field is a display copy even when it copies a command;
the separate command value is preserved.

Reader searches:

```sh
rg -n 'provider_stdout|\.full_detail\(|get\("full"\)|\["full"\]' src --glob '*.rs'
rg -n 'capacity::read|capacity::terminal_label|record_unclassified' src
rg -n 'salvage_failed_task\(|enforce_verify_status' src --glob '*.rs'
```

`full` readers are `TaskEvent::full_detail` in `src/types/task.rs`, show events in
`src/cmd/show/text.rs`, and export timelines in `src/cmd/export.rs`. Other matches
are parser assertions or writes. There is no non-display reader of this key.

The two raw provider_stdout readers remain `src/routing/capacity.rs:58` and
`src/task/lifecycle/failure.rs:28`. CONFIRMED with a refinement: capacity::read is
not reached only through terminal_label. Its production callers are:

- capacity::terminal_label (`src/routing/capacity.rs:71`): guarded by status=Failed.
- settle_capacity (`src/cmd/run/lifecycle/phases.rs:130`): guarded by status=Failed.
- record_execution_failure (`src/cmd/run/process.rs:96`): current execution failure;
  invoked after failed completion in record_completion, not historical successful tasks.
- salvage_failed_task (`src/task/lifecycle.rs:122`): failure-transition side effects.
  record_unclassified is called only here; mark_failed, failed settlement/reaper,
  preparation failure, and failed verification of the current run lead here.
  enforce_verify_status is called from run verification/settlement, not a history read.

No reader was found that requires these raw values for an old task still in done or
merged. Therefore those statuses can trim provider_stdout. Failed, stopped, and
skipped retain it. The UPDATE rechecks the captured task status as well as the
original metadata and cutoff, so a task changing from successful to failed between
preparation and commit keeps its evidence.

Both marker keys are recognized by shared event helpers used by show, export, web,
and TUI. The original detail and every byte outside replaced JSON strings remain
unchanged, including number spellings, whitespace, and escaped protected values.
Commands, file lists, accounting/model/token/cost fields, input/delivery/liveness,
backup, billing/routes, quota, completion/verification, and custody/acceptance fields
are preserved. Non-string payload values and malformed JSON are preserved; duplicate
top-level occurrences of a payload key prevent trimming that key.

## Implementation choices

- Store extensions live in new retention modules, wired through store/mutations.rs;
  no store/mod.rs, schema, migration, or status_mutations.rs changes.
- Scan by increasing event row ID, at most 200 rows per read. Prepare replacements
  outside write transactions. Commit at most 200 changed rows or about 512 KiB of
  original metadata per batch; one larger row is allowed. Eligibility and original
  metadata and task status are rechecked at the keyed UPDATE. Writer admission does not wait.
- Explicit clean has a 10-second budget. Daily automatic retention uses 7 days and
  about 2 seconds, after settlement and backup in post_run_lifecycle. Its deadline
  starts before connection admission; contended Store access yields immediately.
  SQL instructions, chunked reads, JSON parsing and string copying check that deadline.
- The existing update-check.json holds separate manual/daily cursors and daily time.
  A nonblocking flock serializes maintenance and version-cache updates. Atomic rename
  protects the state file; version refresh preserves maintenance fields. A completed
  scan resets its cursor so newly terminal or newly old tasks are revisited.
- VACUUM is explicit. Exclusive SQLite locking covers the active-task check through
  compaction, preventing a launch between admission and VACUUM. All nonterminal tasks
  cause refusal. Free-space detection must succeed and allow twice the file size.
  A checkpoint makes the reported post-compaction main-file size meaningful in WAL mode.
- The guide's maintenance reference documents retention, budgets, markers, cadence,
  free space and --vacuum; configuration links it and command-index covers the behavior.

## Measurement

Initial conservative baseline: 1,057 rows; 86,969,053 payload bytes;
95,361,868 metadata bytes saved; 624,398,336-byte file after VACUUM.

Revised command: `python3 /private/tmp/measure-event-retention-success.py`.
The script uses SQLite backup from a mode=ro source connection into a fresh
`/private/tmp/aid-retention-success-measure.db`; it refuses an existing destination.
It preserves other metadata bytes, applies both status policies, and commits batches
at 200 rows or about 512 KiB of original metadata, matching the Rust caps. It measures
the full eligible history without a run deadline. It is a measurement harness, not
execution of the Rust implementation or a regression test.

At cutoff 2026-10-16T06:24:34.038553+00:00:

| Metric | Result |
| --- | ---: |
| Trimmed rows | 3,962 |
| Original payload UTF-8 bytes | 127,648,630 |
| Metadata bytes saved, including JSON escaping | 139,627,288 |
| Database file before | 734,744,576 bytes |
| Database file after VACUUM | 579,264,512 bytes |
| File bytes reclaimed | 155,480,064 |
| Measurement batches | 202 |
| Slowest measured batch | 16.46 ms |
| Trim plus VACUUM duration | 2.87 seconds |
| Trimmed done / merged / failed rows | 3,839 / 17 / 106 |
| Trimmed full / provider_stdout values | 2,744 / 1,599 |

## Guide correction

CONFIRMED: --vacuum was already documented in the bundled command index. The
reported generated-facts failure came from the stale --older-than entry in
UNDOCUMENTED_FLAGS. That entry is removed; no gap is added. GUIDE_FILES now bundles
maintenance.md, which configuration.md linked but the initial installer omitted.
The existing init E2E asserts that this reference is installed. The maintenance
reference now describes both status policies; command-index remains complete.
No new prose-pinning assertion is introduced.

## Initial verification and limits

Base HEAD: f5aba0e165e8bf52a0269a8f5672801c4d29f73c. No remote tested HEAD exists.

- `rbox status`: FAIL, `rbox: Tailscale API POST failed`.
- `rbox ensure grok-bot-chief --lock-timeout 60`: FAIL, DNS probe reported
  `tailscale status returned no JSON (Expecting value at byte 0)`.
- `aid build check -p aid` (initial incorrect package) and
  `aid build check -p ai-dispatch` (corrected package): BLOCKED before compilation by the remote probe;
  0 compiler diagnostics does not mean compilation passed.
- `aid test --bin aid retention`: BLOCKED by the remote probe; 0 tests ran.
  The wrapper's zero-match failure is not a test-suite result.
- `python3 /Users/ming/.codex/skills/.system/skill-creator/scripts/quick_validate.py
  default-skills/aid-guide`: BLOCKED, `ModuleNotFoundError: No module named 'yaml'`.
- `python3 /private/tmp/check-event-retention.py`: PASS, 23 touched Rust files parsed,
  0 static failures; targeted formatting of new files also passed.
- `git diff --check`: PASS. `security-guard repo .`: PASS, 0 findings.
  The staged-output scan and commit hooks also run before commit.

Added regression coverage includes evidence preservation, terminal/recent/running
eligibility, threshold/idempotence, bounded transaction seams, interrupted batches,
zero-budget restart, dry-run counts/nonmutation, daily cadence/cache preservation,
show/export/web/TUI markers, active-task VACUUM refusal, disk admission, and idle shrink.
CLI integration tests exercise dry-run stdout and show/export after trimming.
These tests have not executed because the build box is unreachable. Final-commit
remote crate checks, the configured bin suite, clean_retention_e2e, and official-guide
integration checks remain required. No local compilation or replacement test environment
was used. No sub-tasks or independent cross-review ran, honoring the no-dispatch brief;
cross-review remains outstanding. Startup-lock, launch, migration and unrelated reader
optimizations from the investigation are out of scope and unchanged.

## Revision verification

KB consulted again: `~/Develop/knowledge/eng-knowledge-base/kb event metadata retention successful task`.
The search returned rowid-retention guidance and unrelated matches; no successful-task
payload-retention implementation was found.

The operator reports a44e3a5d full `--bin aid`: 3,585 passed, 1 failed, at
cmd::init::official_guide::tests::official_guide_covers_generated_facts. That result
belongs to the prior commit; no captured remote log was supplied in this follow-up.
The exact stale entry was independently reproduced with the guide tokenization rule.

Revision commands:

- `rbox status`: FAIL, `rbox: Tailscale API POST failed`.
- `rbox ensure grok-bot-chief --lock-timeout 60`: BLOCKED, DNS probe:
  `tailscale status returned no JSON (Expecting value at byte 0)`.
- `aid build check -p ai-dispatch`: BLOCKED before compilation by the same probe.
- `aid test --bin aid retention`: BLOCKED before execution by the same probe; 0 tests ran.
- `aid test --bin aid cmd::init::official_guide`: BLOCKED by the same probe; 0 tests ran.
- `python3 /private/tmp/check-event-retention.py`: PASS, 9 changed Rust files parsed,
  0 static failures; this is not compilation.
- `python3 /private/tmp/check-retention-guide.py`: PASS, 0 stale flag gaps; both clean
  flags documented and maintenance reference bundled.
- `python3 /private/tmp/measure-event-retention-success.py`: measurement results above.
- `git diff --check`: PASS. `security-guard repo .`: PASS, 0 findings.
  `security-guard staged --repo .` and the mandatory hooks run before commit.

New regression tests cover done, merged, failed, stopped, and skipped separately;
nonzero-exit full values, provider-only payloads, full without command provenance,
command-copy full values, both threshold sides, exact protected metadata bytes,
idempotence, and a successful-to-failed race. Existing tests retain conservative
coverage using failed-task fixtures. Shared event helpers also test provider-only markers.
No local compilation or sub-task dispatch occurred. Final-commit remote verification
and cross-review remain pending. The auto-trim hook, cadence, budgets, schema, and
VACUUM protocol are unchanged.

## Audit fixes: mutex scope and retained output

KB consulted: `~/Develop/knowledge/eng-knowledge-base/kb retention mutex deadline oversized payload`.
The search returned cursor-retention and deadline-measurement guidance; no direct fix matched.
The independent audit reported two P2 findings against af150463. Both premises were
CONFIRMED in source: the Store guard spanned trim(), and full_detail returned only a
marker if either payload key was trimmed.

Changes:

- trim_event_payloads starts a shared deadline before any lock acquisition. with_conn
  uses try_lock: contention returns an incomplete report immediately. Each candidate
  read or write transaction obtains and releases its own Store guard. JSON parsing,
  replacement selection and copying run after release.
- Candidate reads use octet_length to inspect large values without materializing them.
  Metadata larger than 64 KiB is loaded through read-only incremental SQLite blob
  reads, closing the handle and dropping the guard after each chunk. Interrupted
  rows do not advance the cursor. Parsing, UTF-8 validation and copying check the
  deadline in bounded chunks; SQLite's progress handler interrupts long statements.
  Temporary busy timeout is zero and both it and the progress callback are restored
  before the connection is released. No dependency feature or schema changes.
- Write counts are published only after commit, so interrupted/rolled-back batches
  cannot inflate reported rows or bytes. VACUUM code moved unchanged to a sibling
  module to keep the retention file under 300 lines.
- full_detail now returns retained full text, or the original detail when full is
  absent/trimmed, plus a key-specific marker. provider_stdout trimming never replaces
  retained full/detail content. Markers distinguish full, provider_stdout, or both.
  Show, timeline export and backup export share this renderer. Compact show uses
  the same key-specific notice. The guide describes these behaviors.

Regression coverage adds held-mutex admission, event writes during JSON parsing,
oversized-row interruption/resumption, SQL deadline interruption and timeout
restoration, deadline checks during scans/copies/UTF-8 validation, partial-marker
rendering combinations, and real CLI full-show/Markdown/JSON export preservation.

The wall-clock deadline includes connection admission and computation; there is no
unbounded mutex or SQLite busy wait. Deadline checks are cooperative at bounded
chunks/SQLite instruction intervals. OS scheduling and an individual filesystem
operation cannot be preempted by an application deadline.

The earlier copy measurement and key inventory remain valid: payload eligibility
is unchanged. No new copy measurement was required for these two fixes. The same
post-settlement daily hook remains; no task-directory file was touched. No sub-task
was dispatched. Remote verification is expected to run after this commit.

Audit-fix verification (no remote tested HEAD): `aid build check -p ai-dispatch`
and `aid test --bin aid retention` both stopped before compilation/execution:
`rbox: wake grok-bot-chief failed: network error; continuing`, then
`rbox: dns ... tailscale status returned no JSON (Expecting value at byte 0)`.
No tests ran. `python3 /private/tmp/check-event-retention.py` passed for 12 changed
Rust files (0 static failures); `python3 /private/tmp/check-retention-guide.py`
passed. `git diff --check` passed. Staged credential scanning runs before commit.
