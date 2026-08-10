# Design: the task success contract

Status: design, approved to implement. Supersedes the parked branch
`fix/verify-infra-classification`, which is re-derived onto this contract in P4
rather than merged.

Source of the problem: `docs/investigation-failed-tasks-20260809.md` §3, and the
third audit `docs/audit-verify-classification-r2-20260809.md`, whose items 5–14
are the consumer map used below. That map is not re-derived here.

## 1. The problem

`TaskStatus` is one axis asked to answer three different questions. Roughly
twenty consumers read `Done`/`Merged` as "this succeeded" without ever looking at
verification. There is no value aid can store that means *we do not know*, so
every unknown is written down as either success or failure.

The two leak directions are both live:

- **unknown → success.** A task whose verification never produced an answer is
  `Done`, and every counter, gate, chart and webhook reads `Done` as success.
- **unknown → failure.** A task whose verify command died before it could test
  anything is `Failed`, and the change gets blamed for the tooling. This is the
  sccache incident of 08-08/08-09 that started the work: eight tasks marked
  FAILED after their agents had run the suite green.

## 2. Measurements

`~/.aid/aid.db`, 7,265 tasks, measured 2026-08-09. Cells that every
status-reading consumer currently gets wrong:

| cell | all-time | since 07-26 | read today as |
|---|---:|---:|---|
| `done` + verify `failed` | 366 | 7 | success |
| `merged` + verify `failed` | 12 | 0 | success |
| `done` + real verify cmd + verify `skipped` (no result recorded) | 87 | 4 | success |
| `failed` + verify `passed` | 77 | 17 | failure |
| `failed` + verify `failed`, Aug (includes the sccache infra deaths) | 32 | 32 | the change is broken |

Two findings that shape the design:

- **`done|failed` is mostly historical but not closed.** Monthly counts are
  111/136/23/96/0 for Mar–Aug; `enforce_verify_status` (landed 2026-03-19) flips
  `Done + verify Failed` to `Failed`, and by August it holds — but 7 rows leaked
  after 07-26. The derivation must handle the cell regardless, because it must
  re-read 378 historical rows correctly at query time.
- **`Pending` has zero persisted rows** in 7,265 tasks. The audit's item 14
  inconsistency is real in code and has no data footprint; it is a cheap code
  fix, not a migration.

`VerifyStatus::Skipped` is the column default (`src/store/schema.rs:194`), so its
true meaning is *no result was recorded*, not *no verification was required*. The
`verify` column is what distinguishes them, and it is the only disambiguator that
works for both legacy and new rows.

## 3. The three axes

| axis | authority | question |
|---|---|---|
| **A. Lifecycle / integration** | `TaskStatus` | Did the run terminate, and is it merged? |
| **B. Verification** | `VerifyStatus` | Did verification run, and what did it say? |
| **C. Judgment** | `TaskOutcome` (derived) | Should a consumer treat this as success? |

`TaskStatus::Done` means **delivered** — the agent finished and the artifacts are
in custody. It does not mean the work is good, and after this change no consumer
asks it that question.

Axis B gains one value, `InfrastructureFailure`, and gains a grouping:

- `Passed` — answered yes
- `Failed` — answered no
- `TimedOut`, `InfrastructureFailure` — **could not answer**
- `Skipped` — no result recorded (see §2)
- `Pending` — a result is expected and has not arrived yet

Axis C is a small enum derived from A and B. It is the only thing a consumer is
allowed to ask "did this succeed".

```rust
pub enum TaskOutcome {
    Verified,                      // delivered, verification answered yes
    Delivered,                     // delivered, no verification was required
    Unverified(UnverifiedReason),  // delivered, verification could not answer
    Broken,                        // delivered, verification answered no
    Failed,                        // the run did not deliver
    Stopped,                       // terminated by the operator
    Skipped,                       // never ran
    InProgress,                    // not terminal
}

pub enum UnverifiedReason { TimedOut, Infrastructure, NoResult }
```

`Verified` and `Delivered` are the only success outcomes. `Unverified` is neither
success nor failure and must never be silently folded into either.

## 4. The derivation

One function, `TaskOutcome::derive(status, verify_status, verify_required)`,
exhaustively matching both enums. `verify_required` comes from one shared helper
reading the `verify` column: `None | "" | "none" | "false" | "skip"` → not
required; anything else, **including `"auto"`**, → required.

Collapse rules (they consume no verify value):

| status | outcome |
|---|---|
| `Waiting`, `Pending`, `Running`, `AwaitingInput`, `Stalled` | `InProgress` |
| `Failed` | `Failed` |
| `Stopped` | `Stopped` |
| `Skipped` | `Skipped` |

The table that matters — `Done` and `Merged` map identically, because axis A
already carries the merged/not-merged distinction:

| verify_status | verify required | outcome |
|---|---|---|
| `Passed` | either | `Verified` |
| `Failed` | either | `Broken` |
| `TimedOut` | either | `Unverified(TimedOut)` |
| `InfrastructureFailure` | either | `Unverified(Infrastructure)` |
| `Skipped` | yes | `Unverified(NoResult)` |
| `Skipped` | no | `Delivered` |
| `Pending` | yes | `Unverified(NoResult)` |
| `Pending` | no | `Delivered` |

Two shape rules, both non-negotiable, both aimed at the recurring defect where a
rule is fixed at the site it was reported and keeps biting everywhere else:

1. **Allowlist.** Only the cells enumerated above are success. Any new variant on
   either axis is a compile error in the exhaustive match, and defaults to
   non-success once written.
2. **Golden table test.** A single test enumerates the full cartesian product of
   `TaskStatus × VerifyStatus × verify_required` and asserts the outcome for every
   cell. Adding a variant breaks the table and forces a deliberate decision per
   cell instead of an inherited default.

### `Pending` is the in-flight marker

A task dispatched with a verify command stores `Pending` from the moment it is
inserted, and verification overwrites it with a result. This gives `Pending` the
meaning the design assigns it — asked, no answer recorded yet — and closes a
race that was live in the P1 waiter: a task flips to `Done` in the runner, then
post-run work including the `after_complete` hook runs arbitrary shell commands,
and only then does verification start. In that window `verify_status` was still
the column default, so `aid watch --wait` derived `Unverified(NoResult)` and
reported failure for a task that was merely mid-verification — the contract's
own defect pointing the other way. The waiter now waits while verification is in
flight, bounded by the verify timeout so it cannot hang.

Legacy rows keep the old shape: they have no `Pending` and are read through the
`verify` column exactly as the table says.

### The `auto` ambiguity, removed rather than accepted

An earlier draft accepted that `verify = "auto"` resolves to a real command or
to `skip` at run time, that the resolved value is not stored, and that such rows
would read as `Unverified(NoResult)`. Adding the `Pending` marker turned that
imprecision into a visible wrong answer, so it is fixed at the source instead:
when the resolved command is `skip`, verification records `Skipped` explicitly
rather than returning without writing. A task that asked for verification and
legitimately needed none now reads as `Delivered`.

## 5. Locked decisions

1. **`TaskOutcome` is derived, never stored.** No column, no migration; the 378
   historical mislabeled rows are re-read correctly at query time. Aggregation
   moves into Rust over raw `(status, verify_status, verify)` — including
   `src/store/queries/task_metrics_queries.rs`. If a query must stay SQL-side,
   its `CASE` expression is generated from the same Rust table constant and an
   equivalence test over the golden cartesian product proves the two agree. The
   rule is never hand-written twice.
2. **`enforce_verify_status` keeps flipping `Done + Failed` → `Failed`, and is
   out of scope for this effort.** Removing it would be axis-pure but would
   silently reclassify verify-failed tasks as delivered for every external reader
   (scripts, MCP clients, aidbar, hiboss) — the same regression direction that
   parked the branch. Revisit only as a separate, announced change.
3. **Gate policy.** `Verified` / `Delivered` proceed. `Unverified` warns and
   requires an explicit flag. `Broken` refuses without an explicit override.
   `Failed → Merged` is already a legal transition (`src/types/status.rs:105`),
   so the override path is precedented; the flag guards a semi-irreversible
   action and is not a decorative gate.
4. **Machine surfaces change additively only.** JSON, MCP and webhook payloads
   gain `outcome` and `verify_status` fields. The meaning of an existing
   `status: "done"` in a payload does not change in this effort.
5. **A success base holds only tasks with an outcome.** `aid stats` used to
   divide by "everything except `Waiting`", which put running, pending and
   stalled tasks into the denominator of a success rate. They have no outcome
   yet, so they leave it. Measured on the 30-day window this moved nothing (base
   1270 either way), but it moves live windows, and it is a decision rather than
   a side effect of routing the question through `TaskOutcome`.
6. **A verification tag appears only when verification has something to say.**
   `Broken` and `Unverified(reason)` are labelled; everything else is not. A
   running task and a task that never had a verify command both carry no tag —
   otherwise the board wears a meaningless marker on the 1,612 rows that are
   simply failed with no verification configured.

### Where each axis answers

`TaskOutcome` collapses `Done` and `Merged`, and collapses every non-terminal
status into `InProgress`. That is right for judgment and wrong for anything
asking *what stage is this in*. Two gates were written the wrong way before this
was explicit: the merge gate stopped refusing an already-merged task (P1), and
the board counted queued tasks as running (P2). The rule: ask `TaskStatus` for
lifecycle and integration, ask `TaskOutcome` for judgment, and when a call site
needs both, ask both.

## 6. Consumers

From the audit report, items 5–14. Twenty sites, classified by the question each
one is actually asking.

**Class G — gates (must not let a non-success silently pass)**

| # | site | current defect |
|---|---|---|
| G1 | `src/cmd/merge.rs:57-66,203-205` | accepts `Done` without checking verification |
| G2 | `src/cmd/merge_lanes.rs:48-50` | same, for GitButler lanes |
| G3 | `src/cmd_dispatch.rs:84-124` | `RunExitStatus` special-cases only `Done`; a `Merged` inconclusive task gets generic "failed" wording |
| G4 | `src/cmd/wait.rs:78-89` vs `src/cmd_dispatch.rs:140-147` | `Done + Pending` returns success from `wait` and failure from foreground dispatch |
| G5 | `src/cmd/run_lifecycle.rs:184-187` | recommends merge for any `Done` |

**Class C — counters (must not count a non-success as success)**

| # | site |
|---|---|
| C1 | `src/board.rs:181-187`, `src/cmd/board_stream.rs:202-205,274-279` |
| C2 | `src/cmd/stats.rs:40-52`, `src/store/queries/task_metrics_queries.rs:73-104` |
| C3 | `src/state.rs:114-121` |
| C4 | `src/usage.rs:368-376` |
| C5 | `src/cmd/config_display.rs:174-211` |
| C6 | `src/tui/charts.rs:103-110` |
| C7 | `src/agent/selection.rs` |
| C8 | `src/cmd/summary_cli.rs:15-22,92-117` |
| C9 | `src/cmd/batch_validate.rs:145-159` |

C7 has an extra rule: `Unverified` is excluded from **both** numerator and
denominator of agent scoring. An sccache death is not evidence about the agent,
and counting it either way is a lie about the route.

**Class R — reports (must render axis B whenever the outcome is not `Verified` or `Delivered`)**

| # | site | current defect |
|---|---|---|
| R1 | `src/cmd/show_output_brief.rs:27`, `src/cmd/show.rs:350-353` | prints `Status: DONE` only |
| R2 | `src/board.rs:101-110` | has `[VINFRA]`, omits `TimedOut` |
| R3 | `src/cmd/mcp_tools.rs:130-136,281-291` | `status: done`, no verification field |
| R4 | `src/cmd/watch_stream.rs:106-130` | emits `task_done`, counts zero failures |
| R5 | `src/cmd/run_lifecycle.rs:163-182`, `src/cmd/show_json.rs:105-122`, `src/webhook.rs:17-54` | hooks and webhooks report "completed", no `verify_status` |
| R6 | `src/tui/ui_helpers.rs`, `src/tui/ui_tree.rs`, `src/tui/charts.rs` | status label only |

Already correct, keep as regression anchors: `aid show --json`
(`src/cmd/show_json.rs:55-80`), board JSON (`src/cmd/board.rs:273-290`), the web
API (`src/web/api.rs:100-143`) — all three already expose raw `verify_status`.

## 7. Phases

Each phase is independently shippable and independently cross-audited. A
half-converted contract is acceptable only because every phase leaves the
derivation total and every unconverted consumer behaves exactly as it does today.

- **P0 — contract only.** `TaskOutcome`, `UnverifiedReason`, `derive()`,
  `verify_required()`, golden cartesian table test. **Zero consumer changes, zero
  behavior change.** Reviewable as a pure addition.
- **P1 — Class G.** Gates and exit codes, including the `Pending` wait/foreground
  inconsistency. Highest value: stops unverified work merging silently.
- **P2 — Class C + R2.** Counters ship together with board row rendering, so when
  the numbers change the rows on screen explain why.
- **P3 — Class R.** Remaining display and machine surfaces, additive only.
- **P4 — verification evidence boundary.** A verify process that starts and
  returns zero passes; one that returns non-zero fails; a timeout is
  inconclusive; and a spawn/read/wait error is an infrastructure failure.
  Verification output wording never reclassifies the process result.

No data migration in any phase.

## 8. What this does not do

Attribution (axis C in the earlier sketch — whose fault a failure is) stays
minimal: `UnverifiedReason` is the whole of it, consumed only by the C7
exclusion. No blame model, no per-agent fault ledger. That is a separate design
if it is ever wanted.
