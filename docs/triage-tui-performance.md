# Investigation: `aid watch --tui` input stalls

Date: 2026-09-05. Installed binary: `aid 10.40.0 (39a30b8f)`.
The TUI and latest-event query source matched that revision during this audit.

## Confirmed cause in v10.40.0

The main input/render loop synchronously reloads the task snapshot every two
seconds. Its latest-event query repeats a task's maximum-timestamp search for
each historical event belonging to that task. The database has no index covering
`(task_id, timestamp)`. This makes work grow approximately with the sum of squared
event counts per selected task, even though the UI needs only one event per task.

While that query executes, the same thread cannot process keys or draw frames.
Opening all history multiplies the query workload and also exposes expensive
full-task cloning and row construction on every frame.

## Installed-binary reproduction

The installed binary was exercised through a 220-column, 50-row pseudoterminal
using an isolated copy of the database. The fixture has no worker job files;
active task rows were marked done in that copy to prevent startup maintenance
from interacting with live work. The default visible task set remained 38 tasks.
Updates were disabled and a fresh price cache was supplied. The live database,
running agents, and installed binary were not modified.

| Interaction | Time to first frame | Time from `q` to process exit |
|---|---:|---:|
| Quit while idle | 1,041 ms | 7.1 ms |
| Press `r`, then `q` 50 ms later | 967 ms | 360.9 ms |
| Press `a` for all history, then `q` 50 ms later | 2,947 ms | Still blocked after 16,780 ms; diagnostic process stopped at the 20-second overall limit |

These are individual reproductions, not percentile estimates. They establish
that a refresh blocks real keyboard input in the optimized installed binary.
No live TUI process was available to sample when the investigation began; the
five existing aid processes were background workers.

## Dataset and query evidence

The frozen snapshot contained:

- 10,816 tasks and 461,229 events; database size 479,629,312 bytes.
- 38 tasks in the default today-plus-active scope, with 4,347 events.
- The heaviest visible task was already done and had 1,340 events. Its squared
  event count contributes about 83% of the default scope's repeated-scan work.
- The heaviest historical task had 10,451 events. The sum of squared event counts
  grows from 2,160,779 in the default scope to 282,119,285 for all history.
- Original plus resolved prompt text totals about 119.8 million characters across
  all tasks; task-list queries load both fields even though the board shows a
  short prompt preview.

[The pre-fix query](../src/store/queries/event_queries.rs) in
`latest_events_batch` contains:

```sql
SELECT task_id, MAX(id) AS latest_id
FROM events
WHERE task_id IN (...)
  AND timestamp = (
    SELECT MAX(latest.timestamp)
    FROM events latest
    WHERE latest.task_id = events.task_id
  )
GROUP BY task_id
```

`EXPLAIN QUERY PLAN` reports a correlated scalar subquery. The existing event
indexes are `(task_id)`, `(task_id, event_type)`, and `(task_id, id)`; none can
directly answer maximum timestamp. Consequently the inner query rescans each
task's event history for each candidate outer event.

A candidate query performs one lookup per requested task while retaining the
existing timestamp ordering and highest-ID tie break:

```sql
WITH requested(task_id) AS (VALUES (?), (?))
SELECT e.task_id, e.timestamp, e.event_type, e.detail, e.metadata
FROM requested r
JOIN events e ON e.id = (
  SELECT id FROM events
  WHERE task_id = r.task_id
  ORDER BY timestamp DESC, id DESC
  LIMIT 1
);
```

On the unchanged snapshot, using Python's SQLite connection with default
settings, the current query took 4,214 / 1,479 / 1,354 ms; the candidate took
3.80 / 6.21 / 4.13 ms. All five returned fields matched exactly for all 38 rows.
Repeating with AID's mmap/cache pragmas also retained the large gap: current
2,421 / 6,622 / 3,410 ms versus candidate 5.65 / 3.02 / 3.14 ms. Three earlier
read-only attempts against the live database exceeded a five-second diagnostic
limit. These standalone timings are not installed-binary UI latencies.

No index was added and no production SQL was changed for this comparison.

## Native Rust breakdown

The ignored [performance probe](../src/tui/performance_probe.rs) runs the real
Store, App, tree builder, and ratatui TestBackend against the explicit read-only
snapshot. It uses normal database connection pragmas and an isolated AID_HOME.
These measurements came from a **debug test build**, so rendering timings must
not be presented as optimized-release estimates.

| Operation | Measured duration |
|---|---:|
| App initialization | 2,159 ms |
| Current latest-event query | 1,928 / 1,957 / 3,538 ms |
| Candidate query, reading the same five columns | 8.61 / 8.27 / 11.33 ms |
| Milestone batch | 1.22–2.03 ms |
| Workgroups | 9.36–19.11 ms |
| Today's tasks / active tasks | 7.45 / 0.67 ms |
| Default tree construction | 0.41–0.53 ms |
| Default 220x50 render | 27.6–74.2 ms |
| One due refresh tick | 5,063 ms |
| Largest visible task's full event history | 12.2 ms for 1,340 events |
| Load all 10,816 tasks | 3,519 ms |
| Build the all-history tree | 2,731 ms |
| Render all-history board into a 220x50 backend | 2,555 ms |

The candidate probe reads raw column values; the current Store method also maps
timestamps, event types, and metadata into domain values. The independent SQL
comparison above removes that mapping difference and verifies result equality.

Run the native probe explicitly; it is ignored during ordinary test runs:

```bash
AID_TUI_PROFILE_DB=/absolute/path/to/snapshot.sqlite \
  cargo test --target-dir /tmp/aid-tui-profile --bin aid \
  profile_tui_snapshot -- --ignored --nocapture --test-threads=1
```

Use a consistent SQLite backup for the snapshot rather than copying only the
main file of a live WAL database. The probe requires an explicit path and never
opens the normal aid database implicitly.

## Other contributing paths

1. [The event loop](../src/tui/mod.rs) performs drawing, key handling, and
   `app.tick()` on one thread. [Refresh](../src/tui/app.rs) and the `r`/`a` key
   handlers call synchronous reloads. Even a faster query will not eliminate
   stalls caused by unusually large snapshots or database lock waits.
2. [Board rendering](../src/tui/ui.rs) rebuilds the tree and constructs every row
   on each 100-ms frame, including rows outside the viewport. Ratatui's table
   constructor collects the iterator. [Tree construction](../src/tui/tree_data.rs)
   clones entire Task objects, including resolved prompts, and repeatedly scans
   each project's task list when finding descendants. This dominates all-history
   rendering after the query problem is removed.
3. [Multipane refresh](../src/tui/app_refresh.rs) loads histories for every scoped
   task, although only six panes are displayed. Detail/dashboard views also
   reload full event histories instead of fetching new events since a cursor.
   [Detail rendering](../src/tui/ui_detail.rs) clones the event cache each frame;
   its output tab reads task output through `task_view::read_output` on every draw.
4. [Process metrics](../src/tui/app_tasks.rs) reads a background spec and launches
   `ps` separately for each running/awaiting task every two seconds. On five
   current worker PIDs, five sequential queries took 49.8–87.6 ms (median 61.5).
   A single batched query was not faster in this sample (median 82.6 ms), so
   batching alone is not supported as the primary remedy by these measurements.

## Repair order

1. Replace the repeated-scan latest-event SQL with one lookup per requested task.
   Preserve missing-task behavior, out-of-order timestamps, and highest-ID ties.
   Evaluate a `(task_id, timestamp DESC, id DESC)` index separately, including its
   write/storage cost, rather than treating an index as a substitute for the rewrite.
2. Fetch snapshots off the input/render thread using a separate read connection.
   Permit only one refresh in flight; coalesce refresh requests, discard stale
   results, and keep the previous snapshot usable while refresh is underway.
3. Cache tree/row state between task, grouping, collapse, and search changes.
   Store lightweight row references and build only visible row content. Load
   resolved prompts when entering detail rather than in the board listing.
4. Fetch incremental events for visible panes and cache output content by file
   modification state. Move resource sampling off the input thread as well.

## Implemented repair

- Latest events now use one timestamp/ID-ordered lookup per requested task, backed
  by `(task_id, timestamp DESC, id DESC)`. The index occupies 25,886,720 bytes on
  the 461,229-event fixture. Initial creation is a one-time startup migration.
- A dedicated worker reads snapshots through the existing Store handle. The UI
  never queries that handle while refreshing. Only one refresh is in flight;
  intermediate requests coalesce and stale view requests are discarded.
- Tree nodes retain task indices and IDs instead of full prompt-bearing Task
  clones. Cached nodes are shared through Arc, child lookup uses an adjacency
  map, and board/tree widgets format only the viewport's rows.
- Metrics, statistics, detail events and output reads run in the worker. Event
  slices are borrowed during detail rendering; only six pane histories load.
- Refresh errors retain the last snapshot with a visible error. Quit is checked
  before refresh processing. Identity-based selection and collapsed groups survive
  snapshot replacement.

The deterministic blocked-database integration test exercises rapid scope changes,
rendering and quit while a refresh cannot finish. Additional tests cover stale
scope results, selection retention, refresh failure, pane limits, and timestamp
ordering/ties. A real-PTY E2E test exercises historical scope, tree navigation,
and quit during refresh.

An initial debug build against the same 10,816-task terminal fixture exited in
25.54 ms after switching to all history, and 41.15 ms after allowing history to
load. These are individual measurements taken while the regression suite was
running, not release performance estimates.

Full prompt projection and incremental event cursors remain possible follow-up
optimizations: snapshots still load complete task rows off the UI thread, and
visible histories are refreshed in full. The current change removes those reads
from keyboard processing and prevents invisible rows from multiplying rendering
work.

The final native debug probe measured all-history tree construction at 111.7 ms
(previously 2,731 ms), viewport rendering at 21.3 ms (previously 2,555 ms), and
a due UI tick at 0.88 ms (previously 5,063 ms). These timings exercise the frozen
snapshot without the new event index. Task loading still took 2,853 ms in this
debug run, now on the background worker in the interactive application.
