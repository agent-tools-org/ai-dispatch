# Investigation: foreground followers never exit for settled read-only tasks

KB consulted: `kb foreground wait verify pending read-only hang` and
`kb orphan process ps cpu syspolicyd` returned no direct match. The nearest note is
`ai-coding/a-pending-acceptance-criterion-needs-a-denominator.md`, which describes the
same pattern: a pending gate with no owner left to resolve it.

## Symptom (2026-11-10)
- 36 aid processes were alive for up to 7 days after their tasks reached a terminal
  status: 33 foreground `aid run`, 1 `aid wait`, and 2 `aid __run-task` workers.
- Each used about 3.5% CPU, roughly 1.5 cores in total. Every one kept spawning
  `ps -A -o pid= -o ppid= -o pgid= -o time= -o lstart= -o stat= -o args=`, and each new
  process triggered a macOS code-signature check, which kept syspolicyd at about 87% CPU.
- `aid board --running` listed none of their tasks.

## Evidence
- All 33 foreground runs mapped to `done`/`failed` tasks with `read_only = 1`,
  a configured `verify` (`cargo check --all-targets`, project default), and
  `verify_status = 'pending'`. Since 2026-10-20 there are 134 such rows in the store.
- Job specs: most stuck tasks had none. t-7804b5eb and t-91d1b83b had a spec whose
  worker was dead. t-25280046 and t-1eb911f8 had a spec with a live, hung worker.
- The hung workers' `worker.log` was filled with `Skip agent signal for <other task>`,
  which is the global `check_zombie_tasks` sweep running in a loop.
- Both hung workers had in-process retries (t-7b496d1b, t-2f4896c6). Each was read-only,
  failed, and had verify still `pending`.

## Root cause
1. `cmd/run/verify.rs` `maybe_verify` skips verify for read-only tasks
   ("Verify skipped: task is read-only") but never writes a verify status, so the row
   keeps the `Pending` it was created with (`dispatch_prepare.rs`).
2. `cmd/run/foreground_watch.rs` `wait_for_task` returns only when
   `task_is_complete` holds, and that is false while verify is `Pending` and required.
   With no worker left to change the status, the follower loops forever, and every
   100 ms iteration runs `check_zombie_tasks`, which runs `ps -A` once per live task.
3. In-worker retries reach the same loop through
   `retry_or_cascade` → `run()` → `run_foreground_task` → `wait_for_task(retry)`. The outer
   worker never exits, so `aid wait <parent>` sees a live worker and spins as well.
4. Separately, `wait_for_task` never returns when the spec remains but its worker is dead.
5. `aid wait` uses the same Pending gate (`verification_is_in_flight`). It is bounded by
   `VERIFY_TIMEOUT`, after which it reports a successful read-only task as failed.

## Mitigation applied
All 36 processes received SIGTERM after their task was confirmed terminal, and all exited.
Processes following live tasks were left alone.

## Fix
- Both followers share `wait_settlement::worker_is_settling`. A terminal task with no live
  worker holding its spec is settled whatever its verify status, and a spec with a dead
  worker ends the foreground follower with an error.
- The read-only verify skip records `Skipped`.
- `verify_required_for(verify, read_only)` makes read-only tasks derive `Delivered`, not
  `Unverified`, in `Task::outcome`, the metrics/history queries and dispatch creation.

Evidence: the three regression tests failed on the tests-only commit 239a3bfa. The full
suite at 8a3d880c passed on the build box (3804 passed, 0 failed); main at ffd404f4 passed
3797. An independent read-only audit returned SHIP.

## Follow-ups (not fixed here)
- Every follower runs `check_zombie_tasks` every 100 ms. That sweep runs one `ps -A`
  per live task, because `monitor::activity::latest_owned` writes no sample when the root
  has no children and therefore never rate-limits.
- `activity::latest` takes the root's start time from the current process, so a reused
  pid passes the identity check.
- 213 job specs of failed tasks remain in `~/.aid/jobs`.
- A worker spec whose pid has been reused by a live process still reads as settling
  (`worker_is_settling` probes the pid only).
- `cmd::stop::protocol_round6_tests::caller_owned_missing_agent_evidence_does_not_prevent_signalling_the_verified_worker`
  failed in 3/3 suite runs launched by aid's post-task verify, and passed in 4/4 direct
  `scripts/remote-test.sh` runs (alone, its module, and the full suite on main and on the branch).
  The worker `sleep 60` exited 0, so stop never signalled it. Unexplained; it may depend on
  the verify environment.
