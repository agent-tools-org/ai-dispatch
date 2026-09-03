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

## Deliberate choices

The watcher reports non-running status transitions on stderr and preserves the
existing task completion line on stdout. SIGINT stops the worker through the
normal stop path; SIGTERM and SIGHUP leave it running and exit with the existing
reattach hint. PTY execution remains the background worker’s established path,
so interactive agents continue to work through `pty_runner`.
