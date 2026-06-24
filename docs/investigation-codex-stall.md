# Investigation: codex task t-5add2411 hung 32 min, idle watchdog did not fire

## Symptom
Task `t-5add2411` (agent=codex, worktree `agent-delegation`) produced its last
event at 11:35:19 and then sat with zero output for ~32 minutes until the user
manually `aid stop`-ped it at 12:07:29. Final status: `stopped` (user), NOT
`failed`/hung. No "hung detected" event was ever written.

## Root cause of the codex-side stall (NOT an integration bug)
codex `0.141.0` ran 5 read-only commands (sed/rg/git status), recorded all
outputs, then sent the next turn to the model API and blocked waiting for a
streaming response that never arrived.

Evidence (both froze at the SAME instant, 11:35:19, then total silence):
- codex rollout log `~/.codex/sessions/2026/06/24/rollout-...-019ef7e8....jsonl`
  ends with 5 `function_call_output` + a final `token_count`. mtime 11:35:19.
- aid raw stdout log `~/.aid/logs/t-5add2411.jsonl` (39 lines) ends with
  `item.completed` for the last command. No `turn.completed`. mtime 11:35:19.
- Context only 49% used (127825 / 258400). Rate-limit primary 45%. Neither was
  the cause. `--full-auto` so no approval prompt.

aid parsed every event codex emitted; the codex adapter (`src/agent/codex.rs`)
matches the 0.141.0 schema. The codex integration is healthy.

Secondary cosmetic note: the board/TUI showed the 5 commands stuck
`in_progress` because `parse_command_event` drops `item.completed` for plain
read commands (`classify_output` returns `None` for non test/build/lint output).
That is display-only, not the hang.

## The REAL aid defect to fix
aid HAS a 300s idle watchdog:
`src/watcher.rs:71-89` — `timeout(idle_timeout, lines.next_line())`; on elapse
it force-kills the process group, writes a "hung detected" event, sets
`TaskStatus::Failed`. `idle_timeout` defaults to `DEFAULT_IDLE_TIMEOUT` = 300s
(`src/idle_timeout.rs:8-9`) and is passed `Some(idle_timeout)` from BOTH
`src/cmd/run_agent.rs:102` and `src/cmd/run_process.rs:209`. `HUNG_TIMEOUT`
fallback is also 300s.

So on paper this task should have been killed at ~11:40:19. It was not. stdout
was confirmed silent for 32 min (>> 300s), yet no hung event, no Failed status.

=> The watchdog did not protect this run. The most likely mechanism (to be
confirmed, not assumed): the aid runner process that owns the `watch_streaming`
loop was no longer alive/polling between 11:35 and 12:07 — e.g. a worktree/
background dispatch path where codex gets orphaned if the supervising runner
exits, leaving no one to enforce the idle timeout. When the runner is gone, the
`timeout(...).await` simply isn't being polled, so it never fires; the DB task
stays `running` until a human `aid stop` kills the orphan and writes `stopped`.

## What to verify before fixing
1. How was this task dispatched/supervised? Trace the worktree dispatch path:
   `src/background.rs:170` (pty_runner) vs `:184` (cmd::run::run_agent_process),
   and which one codex+worktree takes. Confirm whether the watchdog future lives
   only inside a process that can exit while codex keeps running.
2. Confirm there is no independent reaper that enforces idle/heartbeat for tasks
   in `running` state based on last-event timestamp. (`src/unstick.rs` only
   marks stalled / nudges — check whether anything actually KILLS.)
3. Determine the minimal, correct fix (do not over-engineer).

## Confirmed mechanism

The hung task was not protected by an independent idle reaper. Background
dispatch persists a job spec and spawns `aid __run-task` (`src/cmd/run_dispatch_execute.rs:105-142`).
That spec currently sets `interactive: true`, so the detached worker takes the
PTY path in `src/background.rs:171-181`, not the non-PTY `watch_streaming` path
at `src/background.rs:182-197`.

The PTY path does have an idle watchdog: `src/pty_runner.rs:50-61` passes the
command idle timeout into `src/pty_watch.rs:396-407`, which records hung events
and breaks the monitor after no output. That watchdog only runs while the
detached `__run-task` worker is alive and polling the PTY.

`src/unstick.rs:11-33` only queues nudges or marks a running task `stalled`; it
does not kill the agent or fail the task. The only cross-command cleanup is
`background::check_zombie_tasks`, called from startup and board/watch/wait
paths (`src/main.rs:112-114`, `src/cmd/board.rs:48`, `src/cmd/watch_stream.rs:26`,
`src/cmd/wait.rs:112`). Before this fix it reconciled dead worker PIDs and a
24h maximum runtime (`src/background.rs:423-514`), but it did not compare the
latest task event timestamp against the task idle timeout. If the PTY worker
exited or the persisted supervisor state was incomplete while the agent child
remained orphaned, no live future remained to poll the 300s timeout, and no
other path converted "running but silent" into a hung failure.
