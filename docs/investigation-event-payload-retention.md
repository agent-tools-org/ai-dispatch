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

Only `full` is trimmed, and only when all of these conditions hold:

- Event type is tool_call, build, test, file_read, file_write, lint, or format.
- Metadata has a string command and integer exit_code=0; status is not in_progress.
- The full value is a string containing at least 4,096 UTF-8 bytes and differs from command.
- Task is done, merged, failed, skipped, or stopped, with completed_at older than the cutoff.

Producer proof: `src/agent/codex/events/commands.rs:36` copies aggregated_output into full
alongside command and exit_code. The start path instead records status=in_progress.
Ambiguous full values without this output provenance are preserved.

Reader search (production and tests):

```sh
rg -n 'provider_stdout|\.full_detail\(|get\("full"\)|\["full"\]' src --glob '*.rs'
```

`full` readers are `TaskEvent::full_detail` in `src/types/task.rs`, show events in
`src/cmd/show/text.rs`, and export timelines in `src/cmd/export.rs`. Other matches are
parser assertions or writes. New marker readers only change presentation: shared event
helpers, CLI show, web event responses, and TUI event lines say "output trimmed".
The original detail and every byte outside the replaced JSON string remain unchanged,
including number spellings, whitespace, and escaped strings in protected values.

`provider_stdout` is deliberately excluded: `src/routing/capacity.rs:58` reads it for
provider capacity evidence; `src/task/lifecycle/failure.rs:28` reads it for diagnostic
classification. Its large size does not make it safe to trim. Commands, file lists,
reasoning, errors, completion, accounting/model/token/cost fields, input/delivery/liveness,
backup, billing/routes, quota, and custody/acceptance evidence are preserved. Metadata
containing malformed JSON or duplicate top-level full keys is skipped.

## Implementation choices

- Store extensions live in new retention modules, wired through store/mutations.rs;
  no store/mod.rs, schema, migration, or status_mutations.rs changes.
- Scan by increasing event row ID, at most 200 rows per read. Prepare replacements
  outside write transactions. Commit at most 200 changed rows or about 512 KiB of
  original metadata per batch; one larger row is allowed. Eligibility and original
  metadata are rechecked at the keyed UPDATE. Writer admission times out after 25 ms.
- Explicit clean has a 10-second budget. Daily automatic retention uses 7 days and
  about 2 seconds, after settlement and backup in post_run_lifecycle. Long individual
  row reads/parses/commits can overshoot the cooperative deadline.
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

Command: `python3 /private/tmp/measure-event-retention.py`.
The script uses SQLite backup from a mode=ro connection and modifies only
`/private/tmp/aid-retention-measure.db`. It preserves other metadata bytes, follows the
same eligibility policy, and commits 50 updates per batch. It is a measurement harness,
not execution of the Rust implementation or a regression test.

At cutoff 2026-10-16T05:54:12.857694+00:00:

| Metric | Result |
| --- | ---: |
| Trimmed rows | 1,057 |
| Original payload UTF-8 bytes | 86,969,053 |
| Metadata bytes saved, including JSON escaping | 95,361,868 |
| Database file before | 734,744,576 bytes |
| Database file after VACUUM | 624,398,336 bytes |
| File bytes reclaimed | 110,346,240 |
| Measurement batches | 22 |
| Slowest measured 50-row batch | 19.55 ms |
| Trim plus VACUUM duration | 2.57 seconds |

## Verification and limits

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
