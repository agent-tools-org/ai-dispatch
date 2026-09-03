# Foreground Worker Refactor

Foreground `aid run` and `aid retry` now persist and dispatch the same detached
worker used by `--bg`, then attach a terminal watcher until the task and any
automatic retry chain reach their real outcome.

## Deleted

- The in-process foreground runner and its spec guard.
- `run_foreground_signal.rs` and all `should_detach*`/`handle_foreground_detach`
  logic.
- `BackgroundRunSpec::detached`.
- Detached-task adoption and its tests.
- Reaper tests that existed only for deliberate foreground detachment.

## Divergences closed

- Background specs now persist `budget`, `session_id`, and `max_task_cost`.
- Background workers pass budget and session resume into agent commands.
- Codex resume fallback milestones are recorded in workers as they were in the
  former foreground path.
- PTY workers enforce the cost ceiling.
- Retry lifecycle args preserve idle timeout, cost ceiling, budget, session,
  and audit flags, and retries remain background workers.
- Foreground no longer creates a host-side workspace symlink or waits for rate
  limits before dispatch; the worker owns both behaviors just as `--bg` does.
- Foreground workers now double-fork and report the reparented worker PID before
  the watcher attaches, so a caller that kills the foreground process tree
  cannot kill the worker or its agent. Parent-side container startup was
  removed; the worker starts the container once.
- `--timeout` keeps its exact seconds in the worker environment and spec;
  sub-minute values are no longer rounded to a 60-second runtime cap.
- Legacy specs containing the deleted `detached` field remain readable and the
  field is ignored; unknown persisted fields are accepted so upgrades do not
  break board, stop, or unstick. Reaper cleanup warns and skips an unreadable
  spec while continuing with other tasks.

## Deliberate choices

The watcher reports non-running status transitions on stderr and preserves the
existing task completion line on stdout. SIGINT stops the worker through the
normal stop path; non-interactive SIGTERM and SIGHUP leave it running and exit
with the reattach hint, while TTY SIGTERM/SIGHUP preserve stop behavior. PTY
execution remains the background worker’s established path,
so interactive agents continue to work through `pty_runner`.

## Validation

- Full aid binary suite: 2526 passed, 9 ignored.
- Foreground subprocess acceptance: success/failure output and exit status,
  SIGINT stop, and caller process-tree kill survival (3 passed).
- Guide, timeout-policy, background, and foreground-watcher suites passed.
