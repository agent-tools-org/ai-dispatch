KB consulted: `kb 'sqlite database locked concurrent long lived readers checkpoint'`; read
`infra-ops/concurrent-long-lived-readers-prevent-wal-checkpoint.md` and
`rust/sqlite-read-connection-breaks-wal-reset.md` (2026-10-23).

# SQLite lock and growth investigation, October 2026

## Findings ranked by impact

### 1. CONFIRMED: nominal read commands compete for the writer lock at startup

Source inspected: `50d578123275a0be4d6325700f42f758aa186c18`. Runtime reproduction used installed
`aid 11.6.0 (d3242309)`, not a build of this checkout. The supplied size measurements are accepted
inputs: 735 MB, 3,039 tasks, approximately 606 MB of events, mostly tool output in metadata.

`src/main.rs:97` opens the writable store, then runs zombie reconciliation before command dispatch.
There are exceptions: advise uses `open_read_only` (`src/main.rs:77`), and earlier fast paths bypass
this startup. `Store::open` enables WAL before installing its explicit 10-second busy timeout
(`src/store/mod.rs:70`). This is not necessarily a zero-timeout window: rusqlite 0.34 initializes
a 5-second busy timeout (`rusqlite-0.34.0/src/inner_connection.rs:115` in the local registry).
Changing journal mode, recovery, and last-connection WAL cleanup can encounter BUSY; setting WAL
on an already-WAL scratch file succeeded immediately while another connection held its writer lock.

Two overlooked startup writes are especially relevant: `src/store/schema/mod.rs:160` and `:166`
run unversioned `UPDATE tasks` statements on every open. Both scan tasks and require writer admission
even with zero matches. Their errors are discarded. Duplicate-column ALTERs also discard errors;
an existing duplicate column generally fails during preparation without waiting for a writer.

`migrate_backup_url` always executes `BEGIN IMMEDIATE` before checking `user_version`
(`src/store/migrations.rs:137`). Even a fully migrated database therefore needs writer admission.
An error here propagates. Busy timeouts apply to individual lock acquisitions, not a single
end-to-end command deadline. Earlier ignored failures can consume several waits before a later
statement fails. Three concurrent boards failed with error code 5 after 38.684–40.086 seconds
with a continuously held scratch writer. The exact statement responsible for each runtime failure
was not traced; the source establishes multiple admission attempts, not a universal 40-second limit.

WAL permits one writer. The per-process mutex (`src/store/mod.rs:45,92`) serializes that process's
connection only. A worker, web server, MCP server, and CLI each have independent admission attempts.
A write after startup can fail after its own busy wait even if startup succeeded. Continuous writer
turnover can starve another writer without any single transaction lasting the whole command delay.
[SQLite WAL](https://www.sqlite.org/wal.html) describes this concurrency model.

### 2. CONFIRMED: launch transactions include filesystem work and child waiting

`with_launch_guard` performs a no-op task UPDATE, calls a closure, then commits
(`src/store/status_mutations.rs:75–101`). The UPDATE obtains the writer lock. The closure has no
database-imposed duration bound. Its production callers are:

| Caller | Work inside the writer transaction |
|---|---|
| `src/cmd/run/dispatch_execute.rs:116` | Spec write, worker spawn/wait, PID save, failure kill |
| `src/runtime/pty/runner/execution.rs:16` | PTY spawn, activity inspection, PID save, error log |
| `src/cmd/run/process_watch.rs:15` | Spawn piped agent, inspect process activity, save PID spec |
| `src/runtime/codex_app_server/mod.rs:161` | Stderr creation, RPC spawn, activity inspection, PID save |

The background spawn is more than a spawn syscall: `src/runtime/background/spawn.rs:35–41`
reads launcher stdout to EOF and waits for the launcher. Its double-fork path calls `waitpid`
at `:74`. Spec persistence serializes JSON, writes a temporary file and renames it
(`src/runtime/background/spec/mod.rs:56`); PID updates load and rewrite the spec
(`src/runtime/background/process.rs:9`). PTY error logging writes a file
(`src/runtime/pty/runner/command.rs:19,50`); RPC spawn creates stderr at
`src/runtime/codex_app_server/io/mod.rs:42`.

These costs scale with prompt/spec size, filesystem latency, process launch and scheduler load,
rather than total events. A stalled launcher or filesystem can outlast all startup admission waits.

### 3. CONFIRMED: scans and deletion can occupy the writer for growing amounts of work

The complete explicit transaction inventory found by searching production Rust for transaction
constructors and BEGIN is below (unqualified paths are in `src/store/`). Deferred transactions
acquire writer admission at their first write.

| Location | Work before commit; scaling and external work |
|---|---|
| `status_mutations.rs:27` | Keyed status UPDATE, usage reconciliation, optional event serialization/INSERT |
| `src/store/status_mutations.rs:82` | Launch guard described above, or keyed child-stop update |
| `completion_mutations.rs:27` | Keyed completion, usage reconciliation, optional event serialization/INSERT |
| `usage_checkpoint.rs:27` | Event INSERT/checkpoint UPSERT; event bytes/index maintenance |
| `workgroups.rs:45` | Count group tasks, DELETE group; stale-snapshot upgrade risk |
| `workgroups.rs:59` | DELETE events/tasks/group; scales with deleted history/pages/indexes |
| `migrations.rs:139` | Immediate admission; version-zero backup normalization/user_version write |
| `src/cmd/steer.rs:55` | Immediate admission; file check, task metadata scan, event/message inserts |

Workgroup workspace removal happens after commit (`src/store/workgroups.rs:49,68`), not under
the transaction. Cascade deletion can fail foreign-key checks when messages or custody records
still reference tasks (`src/store/migrations.rs:14`, `src/store/schema/sql.rs:90,103`); usage and
read-only-launch tables instead have ON DELETE CASCADE. No full cascade success is claimed here.
The read-then-write group deletion also admits SQLITE_BUSY_SNAPSHOT, which retrying a statement
cannot repair; retry the transaction.
See [SQLite result codes](https://www.sqlite.org/rescode.html#busy_snapshot).

Additional implicit autocommit writers, grouped by responsibility (each execute owns admission
through statement completion, unless called inside a transaction listed above):

| Source | Writes and lock-time drivers |
|---|---|
| `schema/mod.rs:14,27,111`; `schema/sql.rs:4`; `kg_schema.rs:10` | DDL; absent-index scans/sorts |
| `src/store/schema/mod.rs:160,166` | Two full task scans on every open, including zero-match UPDATEs |
| `migrations.rs:94,121` | Scan NULL-task dispatch JSON, collect, then separate keyed backfill UPDATEs |
| `migrations.rs:34,44`; `usage_checkpoint.rs:10`; `read_only_launch.rs:10` | Table/index DDL |
| `task_insert_mutations.rs:23,61,107` | Task/waiting insert/replacement; prompt/config bytes/indexes |
| `task_metadata_mutations.rs:12–157`; `task_profile.rs:21`; `execution_mutations.rs:15,38` | Keyed edits |
| `mutations.rs:47,62,94,122` | Group insert/compensating delete; finding/event inserts |
| `src/store/workgroups.rs:29`, `queries/workgroup_queries.rs:113` | Keyed group/finding edits |
| `memory_mutations.rs:14,64,72,81,111` | Memory insert/revision/counters/delete; revision after read |
| `src/store/kg_mutations.rs:24,33` | Triple insert/invalidation |
| `queries/message_queries.rs:27,72,82` | Message insert/delivery updates; task/content predicate scan |
| `artifact_custody.rs:48,96`; `read_only_launch.rs:22` | Acceptance/durability insert, launch upsert |

The table paths above are relative to `src/store/`. DDL validates schemas; the first overage index
examines event/setup JSON. Keyed edits include metadata/profile/backup/verification/review; cost
scales with payload bytes. Event serialization precedes execute but can be inside a caller transaction.
No other production explicit transaction or direct SQL writer outside these modules and steer
was found. Test fixtures are excluded. Group creation's mkdir is after its autocommit INSERT
(`src/store/mutations.rs:60`); it does not retain that writer lock during mkdir.
Writes also pay storage synchronization and can trigger automatic checkpoint work, increasing
command latency under I/O load. Checkpoint latency is not identical to writer-lock hold time.

On the scratch home, rollback-only measurements with foreign keys enabled gave:

| Statement | EXPLAIN QUERY PLAN | Statement ms; rows |
|---|---|---|
| Startup delivery normalization | SCAN tasks | 325.47; 0 |
| Startup verify normalization | SCAN tasks | 12.35; 0 |
| Backup normalization UPDATE | SCAN tasks; correlated SEARCH events using idx_events_task_kind | 1857.36; 0 |
| Group events DELETE | SEARCH task/id index; subquery SEARCH task workgroup index | 84.02; 7,278 |

Rollbacks took 0.01 ms for the zero-row updates and 0.74 ms for event deletion. These exclude
commit/fsync and are not whole-command timings. No measured real write exceeded the concurrent
failure window. Larger qualifying groups, first-time index builds, version-zero metadata backfills
or slow storage can do so; the size threshold is unmeasured. No production DELETE/UPDATE directly
scanning all events was found: deletion uses task indexes, while its affected history can encompass
the table. Backup normalization scans tasks and relevant milestone/error metadata under its write
transaction. Wrap-up scans all events for one task under its immediate transaction.

### 4. CONFIRMED: several read paths retrieve or inspect unnecessary history

All timings below are milliseconds, three successive executions in the same read-only Python
SQLite 3.53.4 connection to the supplied copy, fetching all result rows. They include text transfer
but exclude Rust JSON parsing. `T` below means
task count; `E(task)` means that task's history, including metadata bytes.

| Query shape / source | Plan summary | ms (first / repeat / repeat) |
|---|---|---|
| Board/MCP tasks, `task_queries/query_sql.rs:34` | SCAN created_at index | 592.35 / 145.49 / 106.17 |
| Task list with SQL LIMIT 50 (comparison only) | Same ordered index scan, stops early | 9.25 / 1.50 / 0.69 |
| Full history, `event_queries.rs:260` | SEARCH timestamp index (task_id=?) | 7.02 / 2.82 / 1.82 |
| Same history without metadata (comparison) | Same SEARCH | 1.68 / 1.39 / 1.12 |
| Latest event / three, `:193,223` | SEARCH timestamp / task_id | 0.10/0.01/0.01; 0.03/0.01/0.01 |
| Awaiting JSON predicate kernel, `:157` | SEARCH timestamp index; residual JSON filter | 1.79 / 1.34 / 1.27 |
| Wrap-up, `src/cmd/steer.rs:75` | SEARCH task/id; residual JSON | 1.88 / 1.20 / 1.19 |
| Route launch, `event_queries.rs:271` | SCAN task_id index + blobs | 85651.23 / 4829.37 / 1741.04 |
| Last verify, `state_queries.rs:87` | SCAN e; SEARCH task PK; sort | 550.63 / 162.01 / 149.39 |
| Narrow stats, `task_metrics_queries.rs:14` | SEARCH created_at index | 68.80 / 15.55 / 14.42 |
| Budget totals, `task_metrics_queries.rs:90` | SCAN tasks | 7.10 / 10.83 / 7.28 |
| Overage, `billing_queries.rs:9` | SCAN partial overage index; SEARCH task PK | 2.68 / 0.01 / 0.00 |
| Effective-dir backfill SELECT, `migrations.rs:99` | SCAN tasks | 2.39 / 3.09 / 3.27 |
| Backup SELECT, `migrations.rs:23` | SCAN tasks; SEARCH task-kind index | 188.30 / 457.45 / 322.11 |

The history sample is `t-4956288e`, the task with most events: 1,379 rows, 2,960,614 returned
text bytes versus 142,537 without metadata. The route lookup used a nonexistent route and still
scanned all metadata. A separate `sqlite3
-readonly` check confirmed its plan and took 3.240 seconds for zero matches. Last-verify used one
recorded repository and returned no match, still scanning events. Batch kernels are representative
single-task queries, not timings of the entire batch implementation or command.

Command implications:

- Board applies its display limit after `list_tasks` loads all matching tasks and prompt fields
  (`src/cmd/board/mod.rs:46`). Milestone/error batches use task-kind indexes; awaiting batches parse
  E(task) metadata (`event_queries.rs:151`). Infrastructure diagnostics load full history merely
  to select one error (`src/ui/board/mod.rs:73`). Stream refresh repeats these costs.
- Show text/JSON and watch single-task load full histories (`src/cmd/show/text.rs:59`,
  `show/json.rs:32`, `watch.rs:77`). `schema_rows.rs:12` parses every metadata value into JSON,
  even where rendering only uses detail. TUI selected histories do the same (`ui/tui/app/updates.rs:15`).
- Stats is not just the narrow stats SQL: `src/cmd/stats.rs:39` loads all tasks then filters;
  `stats/collect.rs:38` calls component cost per selected task. `cost/components.rs:91` reads its
  entire history to find one completion. Cost loads every task and calculates component costs
  before period/group/agent filtering (`src/cmd/cost.rs:34`). Across Codex/Claude tasks this can
  approach reading all their events and metadata, although only accounting fields are needed.
- Web list enriches every selected task with latest three events, including metadata, and awaiting
  JSON (`src/ui/web/api/mod.rs:200–216`); list has no pagination at `:29`. Events API loads full
  history (`:55`). Latest-event batches are bounded per task; they do not scan total events.
- MCP board loads all filtered tasks and runs the reaper (`src/cmd/mcp_tools.rs:166`); MCP show
  delegates to show (`:179`). Wait normally polls keyed tasks/active tasks, but waiting-prompt
  reporting fetches full history (`src/cmd/wait/report.rs:53`). The orphan reaper does likewise
  merely to compute liveness (`src/runtime/background/orphan/mod.rs:62`), during startup too.
- Route admission uses the global metadata scan, then reads histories of matched launch tasks
  (`src/agent/api_route.rs:163`). Project state last-verify is another global events scan;
  ordinary keyed task/history lookups scale with selected history, not total database bytes.

### 5. CONFIRMED: no age-based database retention or automatic compaction

`aid clean` computes an unused cutoff and explicitly retains tasks/events (`src/cmd/clean.rs:42–46`).
It cleans orphan filesystem artifacts, not old database rows. No automatic age deletion, VACUUM,
auto_vacuum configuration or explicit WAL checkpoint was found by the source search. Explicit
workgroup cascade deletion is the exception. The supplied copy reports auto_vacuum=0,
page_count=179381 and freelist_count=0. Deletion makes pages reusable; it does not shrink the
main file with that configuration. WAL checkpointing does not compact live metadata out of it.

Raw provider output has another stored copy: streaming writes provider lines to log files
(`src/runtime/watcher/watch_streaming_with_input/streaming.rs:109`), while event metadata can
attach `provider_stdout` (`src/runtime/watcher/stream/mod.rs:247`). Task paths include transcript,
agent log and output (`src/support/paths.rs:49–69`); coverage differs by agent and stream path.
Byte-identical, complete duplication for all historical tasks was not verified. Full tool payloads
are useful debugging evidence but are unnecessarily read by summary consumers. Preserve compact
completion/accounting, input-delivery, liveness, backup, route/billing and custody evidence before
trimming payloads: terminal tasks remain inputs to show/export/cost/retry and acceptance workflows.

Index assessment: `idx_events_task_id` is a prefix of all three composite indexes and is a candidate
for removal. More strongly, because `events.id` is INTEGER PRIMARY KEY, ordinary task_id indexes
already carry rowid=id as their tie-breaker: `idx_events_task_id_id` duplicates that ordering.
The latest-three kernel actually chose `idx_events_task_id`. Keep one task/id access path plus
the timestamp and task-kind indexes; the latter two serve different ordering/filter needs.
`NORMALIZE_BACKUP_ATTEMPTS_SQL` explicitly names task-kind. A fifth, partial overage index exists
in the supplied copy/current code and is explicitly named by billing; it is not one of the four
general-purpose indexes in the brief. Validate removals against full batch plans and write costs.

### 6. REFUTED as stated: long-lived processes alone do not pin WAL snapshots

The first KB entry equates attached connections with permanent readers. That is too broad for
this implementation. Store queries collect owned results and drop statements/rows before return
(`src/store/queries/event_queries.rs:262–267`); web serialization, SSE waiting, watch polling and
MCP request waiting happen afterward. No explicit long-lived read transaction spanning polling or
network waiting was found. A slow statement does retain its snapshot while stepping rows: the
85.7-second metadata scan demonstrates a potentially long interval. Overlapping slow queries can
prevent checkpoint progress/reset; open idle processes alone do not establish that condition.

A separate temporary WAL probe with three idle open readers allowed TRUNCATE: `(0,0,0)`, zero
WAL bytes. Pinning one reader with BEGIN + SELECT, then committing 20 writes, produced PASSIVE
`(0,20,0)` and 82,432 WAL bytes. Releasing it allowed TRUNCATE and zero bytes. The tuple means
busy/log/checkpointed frames. SQLite's default automatic checkpoint threshold is 1,000 pages;
the 64 MB `journal_size_limit` in `src/store/mod.rs:53` is not a hard active-WAL cap.
See [checkpoint pragmas](https://www.sqlite.org/pragma.html#pragma_wal_checkpoint).

## Reproduction steps and results

Use the supplied `dbinv/aid-copy.db` only read-only. Let `H` be the supplied `dbinv/home` scratch
AID_HOME; never substitute the live home. Hold a Python sqlite3 connection to `H/aid.db` in
autocommit mode, execute BEGIN IMMEDIATE, then use a ThreadPoolExecutor to run three
`subprocess.run(['aid','board','-l','1'], env={**os.environ,'AID_HOME':H}, capture_output=True,
text=True, timeout=180)` calls concurrently. Keep the writer until all calls finish, then ROLLBACK.
All three returned exit 1, `Error: database is locked`, `Error code 5: The database file is locked`.
Observed completion times since launch were 38.684, 40.086, 40.086 seconds; later observations
are upper bounds because futures were collected in submission order. No live database was written.

An independent 100 ms busy-timeout statement probe confirmed the two startup UPDATEs and
BEGIN IMMEDIATE each failed with BUSY under that writer; setting existing WAL mode did not.
The scratch home had user_version=1 and zero eligible absolute-dir backfill updates.

Measurement commands: `python3 /private/tmp/db-lock-measure.py`,
`python3 -u /private/tmp/db-lock-extra.py`, `python3 -u /private/tmp/db-lock-repro.py`,
`python3 -u /private/tmp/db-lock-stages.py`, `python3 /private/tmp/db-lock-checkpoint.py`,
`python3 -u /private/tmp/db-lock-writes.py`. These scratch scripts used stdlib SQLite, EXPLAIN
QUERY PLAN, monotonic timers and rollback for write probes; no compiler/build/tests ran.
Plans/timings above record results, not pass counts from a Rust suite. A later `sqlite3 -readonly`
open of the scratch WAL home failed with CANTOPEN (14); a normal Python open succeeded.
That separate sidecar/access failure was not treated as BUSY or worked around on the live home.

## Recommended fixes ranked by value and risk

1. **High value, moderate risk: version startup migrations and remove steady-state writes.**
   Check schema version before writer admission, recheck after acquiring it, apply migrations
   atomically once; stop swallowing unexpected DDL/BUSY errors. Install timeout before WAL setup.
   Cost: migration/version coordination and concurrent first-open handling. Prove migrated read
   commands finish while another process holds BEGIN IMMEDIATE; prove two first-open migrators
   converge and interrupted upgrades recover. Include old delivery/effective-dir/backup cases.
2. **High value, moderate risk: bound launch critical sections.** Prepare spec/files outside the
   lock and replace the arbitrary closure with a narrow launch/stop coordination protocol.
   Cost: preserving the existing no-launch-after-stop guarantee; simply moving spawn is unsafe.
   Test stop-versus-launch with barriers, failed spawns/PID persistence, and a stalled launcher;
   unrelated store writes must remain responsive. Keep this separate from a reader split.
3. **High value, low/moderate risk: query compact facts directly.** Index route-launch evidence,
   store/query latest accounting and awaiting/liveness facts, project detail-only events where
   appropriate, and push board limits/cost filters into SQL. Cost: projections, backfill and
   timestamp/id tie semantics. Test semantic equivalence, pagination, malformed JSON and old
   rows; use representative large blobs and assert plans avoid global metadata scans.
4. **High storage value, higher risk: explicit payload retention/archive.** Preserve needed facts
   and retrievable artifacts, trim debug payloads in bounded transactions, separately schedule
   compaction with disk headroom. Cost: archival integrity, changed historical output availability,
   maintenance I/O; VACUUM can need substantial temporary space. Test show/export/cost/retry and
   custody after pruning, interrupted batches, measured page reuse and eventual file shrinkage.
5. **Moderate value, low risk: remove duplicate indexes after plan tests.** Choose one task/id
   index, retain timestamp/task-kind/overage paths. Cost: migration/drop work and possible planner
   changes. Test all event batches, latest-three ordering, cascade and backup/billing forced indexes;
   measure insert latency and index bytes before/after on a disposable representative copy.
6. **Diagnostic value, low risk: record statement role, elapsed wait, extended SQLite code and
   WAL frames/size without payloads.** Cost: small telemetry overhead. Test competing writers and
   a pinned reader. Do not use a longer timeout or journal_size_limit as the primary fix.

## Limits and choices

The other machine's SQLite extended code, command, logs, filesystem, concurrent processes,
disk pressure and migration state remain unknown; its root cause is AMBIGUOUS. Recovery,
exclusive-lock, first-time DDL, stale-snapshot and launcher-stall failures are code-supported
possibilities, not reproduced incidents. Cache/engine variance means these timings are not an SLA.
No live task-directory contents or secrets were copied. Only this document changes; no schema,
index, retention, build input or application code changes. No remote HEAD/test counts exist.
No sub-tasks or independent cross-review ran; the explicit no-dispatch scope was preserved.
