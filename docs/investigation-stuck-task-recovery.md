# Investigation: why `aid unstick` / `aid stop` fail to recover stuck tasks

## Symptom
During a 2026-07-16 session, three background tasks (Fix A/B/C) went stuck with
dead worker processes. Recovery attempts failed in sequence:
1. `aid unstick <id>` reported "Sent unstick nudge to <id>" but nothing
   happened — `ps` and log mtimes were unchanged 15s later.
2. `aid stop <id>` on all three succeeded (task marked `Stopped`), but
   redispatching to the same worktree/branch failed with a "worktree is
   locked" error — the `.aid-lock` file was not released.
3. The operator worked around both by manually inspecting lock state and
   redispatching with explicit "continue from the existing diff" prompts
   instead of using `aid retry`, based on a prior belief that `aid retry`
   silently discards uncommitted work.

This doc traces each symptom to its exact code location on v9.3.0
(current `main`, commit `c776bdc`).

## Root cause A — `aid unstick` has no liveness check and always reports success

`src/cmd/unstick.rs:29-41` (default, non-`--escalate` mode):
```rust
reply::run_with_source(store, task.id.as_str(), Some(&body), None, true, 30, MessageSource::Reply)?;
println!("Sent unstick nudge to {task_id}");
```
This calls `reply::run_with_hook` (`src/cmd/reply.rs:62-98`), which:
- Validates only the **DB task status** (`Running`/`AwaitingInput`/`Stalled`,
  `reply.rs:83-89`) — it never checks whether the worker/agent **OS process**
  is actually alive (no `background::is_process_running` / `load_worker_pid`
  call anywhere in this path).
- Writes a message row (`store.insert_message`) and a steer-signal file
  (`input_signal::write_steer`, `reply.rs:93`) — both are inert unless a live
  PTY loop is polling for them.
- Is called with `async_mode = true` (the literal `true` 5th argument in
  `unstick.rs:32-40`), so it returns `ReplyOutcome::Queued` **immediately**
  without ever calling `wait_for_ack` (`reply.rs:98`, `117-146`) — i.e. it
  never waits to see if anything consumed the nudge.

Net effect: `aid unstick` cannot distinguish "the worker is alive and just
slow to respond" from "the worker process is a corpse." Both cases print the
identical success line. The only way to tell them apart today is the
external `ps` + log-mtime check the operator already did by hand. The
`--escalate` flag (`unstick.rs:16-27`, marks the task `Stalled`) is the
correct tool for a dead worker, but nothing in `unstick` detects deadness and
suggests it — the human has to guess which mode to use.

## Root cause B — `aid stop`/`aid kill` never releases `.aid-lock`

`terminate()` in `src/cmd/stop.rs:80-125` — the shared body of `stop`/`kill`/
`stop_retry_tree` — does, in order: kill worker/agent PIDs, kill sandbox
container, `preserve_worktree()` (auto-commit uncommitted changes),
`capture_final_worktree_state`, `task_lifecycle::mark_stopped`, insert an
event, `background::clear_spec(task_id)`. **It never calls
`worktree::lock::clear_worktree_lock`.** The `.aid-lock` file inside the
worktree is left on disk, still naming the now-dead task.

This alone would be recoverable — `lock_record_is_held` (`worktree/lock.rs:
33-49`) has a fallback that consults the store to see if the recorded task is
terminal — **except the fallback needs a `Store` reference, and the
worktree-reuse preflight check doesn't have one**:

- `worktree.rs:96-105` `ensure_worktree_unlocked()` calls
  `lock::check_worktree_lock(path)` (`worktree/lock.rs:22-24`), which is
  hard-wired to `check_worktree_lock_with_store(wt_path, None)` — **`store`
  is always `None`** on this path.
- In `lock_record_is_held`, when both `owner_pid` (the CLI launcher, which
  exits right after dispatching a `--bg` task) and `worker_pid` are dead,
  the code falls to:
  ```rust
  match store {
      Some(store) => task_status_keeps_lock(store, &record.task_id),
      None => true,   // <-- always "still held" when store is None
  }
  ```
  Since `ensure_worktree_unlocked` never has a store, this branch always
  returns `true` — it can **never** self-heal a lock left by a stopped task.
- `ensure_worktree_unlocked` runs at `worktree.rs:175/190/223`, i.e. inside
  the worktree-preparation step of `aid run --worktree <branch>`, which
  executes **before** the store-aware acquisition
  (`try_acquire_worktree_lock_with_store`, called correctly with `Some(store)`
  at `src/cmd/run_dispatch_prepare.rs:169`). The store-aware call would have
  correctly recognized the lock as stale — it never gets the chance, because
  `ensure_worktree_unlocked` already bailed with `"Worktree {path} is locked
  by task {holder} — concurrent access prevented"`.

So the exact sequence the operator hit: `aid stop` kills the process and
marks the task terminal, but leaves a `.aid-lock` file that the *next*
`aid run --worktree <same-branch>` cannot clear itself, because the check
that runs first has no way to ask the store "is that task actually dead?"

Compounding risk: because `terminate()` never touches `worker_pid` in the
lock file, if that PID is later reused by an unrelated OS process before
someone runs `aid worktree prune` or retries, `lock_record_is_held` returns
`true` straight from the direct `process_alive(worker_pid)` check
(`worktree/lock.rs:34-36`) — bypassing the store fallback entirely — and the
lock becomes falsely "held" indefinitely.

Secondary silent-failure note: `preserve_worktree()` (`stop.rs:151-160`)
swallows the auto-commit result — `let _ = crate::commit::auto_commit(...)`
— so if the commit fails (e.g. git lock contention right after a `sigkill`),
the operator gets **no warning** that in-flight edits weren't preserved.

### Existing but non-obvious workaround
`aid worktree prune` (`src/cmd/worktree.rs:67-123`) already does the right
thing: for every aid-managed worktree it reads the lock's `worker_pid` (or
`owner_pid` if no worker_pid), checks `process_alive_check(pid)` directly
(no store needed — pure PID liveness), and deletes `.aid-lock` if dead,
independent of the worktree's age. This is not gated to the task that was
just stopped, isn't surfaced by `aid stop`'s output, and isn't documented as
"run this if a locked-worktree error blocks a retry" — hence the operator
didn't know to reach for it and instead inspected/cleared lock state by hand.

## Correction — `aid retry`'s "discards work" belief is stale
Checked against current `src/cmd/retry.rs` (`resolve_retry_target`,
`save_partial_work`, lines 102-133) and `src/cli/retry_flag_tests.rs`:
- Default (non-`--reset`) retry **auto-commits** any dirty worktree state
  before redispatching in the *same* directory (`save_partial_work` →
  `git add -A && git commit`) — it only discards work when `--reset` is
  explicitly passed (`reset_dirty_worktree`, `checkout . && clean -fd`).
- `aid retry ... --bg` **is** supported today (`RetryArgs.bg`,
  `retry_flag_tests.rs:9` parses `"--bg"`); the older belief that retry has
  no `--bg` and always blocks foreground reflects a fixed-since gotcha
  ([[feedback_aid_retry_foreground_and_budget_window]]), not current
  behavior.
- The real failure mode this session hit is orthogonal: retrying onto a
  worktree whose `.aid-lock` is still (falsely) held by a stopped sibling
  task would fail for the **same Root Cause B reason** as a fresh `aid run`
  — not because retry itself discards anything.

## Fix directions (not implemented — handing off)
1. `terminate()` in `stop.rs` should call
   `crate::worktree::clear_worktree_lock(&task.worktree_path, task_id)`
   after confirming the worker/agent PIDs are dead, mirroring what
   `preserve_worktree` already does for auto-commit in the same function.
2. `ensure_worktree_unlocked` should take an `Option<&Store>` (threaded down
   from its callers, which already have one available one layer up) and use
   `check_worktree_lock_with_store` instead of the store-less variant, so a
   terminal task's lock is recognized as stale at the same point
   `try_acquire_worktree_lock_with_store` would recognize it.
3. `aid unstick` (nudge mode) should check `background::load_worker_pid` +
   `is_process_running` before sending, and either fail fast with a
   suggestion to use `--escalate`/`aid stop`, or itself escalate
   automatically when the worker is confirmed dead.
4. `preserve_worktree`'s swallowed `auto_commit` error should surface a
   warning (`aid_warn!`) on failure instead of a silent `let _ =`.

Related memories: [[project_arch_audit_2026_07]] (the 2026-07-08 P1 audit —
none of its four shipped fixes cover this `.aid-lock`/`ensure_worktree_unlocked`
gap), [[project_idle_watchdog_pty_blindspot]], [[feedback_aid_retry_foreground_and_budget_window]].
