# Spec: `aid reply` + `aid unstick` (A+B)

## Problem
- `aid steer` writes a one-shot message to PTY stdin with no persistence, no confirmation, no history.
- No mechanism detects or recovers tasks that hang (idle in `Running` state, not `AwaitingInput`).

## Solution

### A) Persistent messaging (`aid reply`)

New table `task_messages`:
```
id INTEGER PRIMARY KEY
task_id TEXT NOT NULL REFERENCES tasks(id)
direction TEXT NOT NULL CHECK (direction IN ('in','out'))  -- 'in' = to agent, 'out' = from agent
content TEXT NOT NULL
source TEXT NOT NULL  -- 'reply' | 'steer' | 'unstick-auto' | 'agent-ack'
created_at DATETIME NOT NULL
delivered_at DATETIME                                       -- when PTY monitor wrote to stdin
acked_at DATETIME                                           -- when agent produced output after delivery
```

New CLI command:
```
aid reply <task-id> <message>           # blocks up to 30s for ack, exits with ack status
aid reply <task-id> -f msg.md           # read from file
aid reply <task-id> "msg" --async       # fire-and-forget (matches current steer semantics)
aid reply <task-id> "msg" --timeout 60  # custom ack timeout
```

Behavior:
1. Insert row (direction='in', source='reply', created_at=now).
2. Write input_signal (reusing existing steer signal file mechanism).
3. PTY monitor picks up → writes to stdin → UPDATE delivered_at.
4. PTY monitor's next-output detection sets acked_at after first new output line following delivery.
5. CLI polls DB until acked_at set or timeout; prints status line.

`aid steer` becomes a thin wrapper: calls the reply handler with `--async` + source='steer' for backward compat. Existing callers keep working.

### B) Idle detection + `aid unstick`

Policy struct `IdleDetector` in `src/unstick.rs` (pure logic, no I/O):
```rust
pub struct IdleDetector {
    pub warn_after: Duration,     // default 180s
    pub nudge_after: Duration,    // default 300s  → auto-reply a nudge
    pub escalate_after: Duration, // default 600s  → mark Stalled
}

pub enum IdleAction {
    None,
    WarnEvent,        // emit "idle warning" event only
    SendNudge(String),
    Escalate,         // mark task Stalled, emit event
}

impl IdleDetector {
    pub fn tick(&self, last_output_at: Instant, state: TaskStatus, already_nudged: bool) -> IdleAction { ... }
}
```

Wired into `pty_watch::monitor_bridge` — on each loop iteration, call `detector.tick(...)`; apply action.

New CLI command:
```
aid unstick <task-id>                   # manual unstick — sends default nudge
aid unstick <task-id> -m "hint message" # custom nudge
aid unstick <task-id> --escalate        # skip nudge, go straight to Stalled
```

New task status `Stalled` (distinct from `Failed` — user can still `reply` or `retry`).

Config (project.toml or run flags):
```
[defaults]
auto_unstick_enabled = true
auto_unstick_warn_secs = 180
auto_unstick_nudge_secs = 300
auto_unstick_escalate_secs = 600
auto_unstick_nudge_message = "Task appears idle. Status update please?"
```

Per-task override: `aid run ... --no-auto-unstick` or `--auto-unstick 120,240,600`.

## File layout

| New / changed file | Purpose |
|---|---|
| `src/store/migrations.rs` | + migration for task_messages table |
| `src/store/queries/message_queries.rs` | insert_message, list_messages, mark_delivered, mark_acked, pending_for_task |
| `src/types.rs` | + TaskMessage, MessageDirection, MessageSource, IdleAction; `TaskStatus::Stalled` variant |
| `src/cli/command_args_b.rs` | + ReplyArgs, UnstickArgs |
| `src/cli/mod.rs` | + Cmd::Reply, Cmd::Unstick |
| `src/cmd/reply.rs` | handler: insert message, signal, wait-for-ack loop |
| `src/cmd/unstick.rs` | handler: send nudge via reply or mark escalated |
| `src/cmd/steer.rs` | delegate to reply with source='steer', --async |
| `src/unstick.rs` | pure IdleDetector policy + tests |
| `src/pty_watch.rs` | consume messages via reply signal, call IdleDetector each tick, emit events, track last_output_at |
| `src/input_signal.rs` | extend to message-aware queue (support multiple pending messages per task) |
| `tests/reply_e2e_test.rs` | E2E: spawn mock PTY agent, reply, observe ack, idle timeout → nudge |

## Non-goals
- Non-PTY agent support (API/background-only agents) — deferred to future C.
- Messages from agent back to caller besides ack detection — future work.
- Message thread UI in TUI — separate task.

## Constraints
- Project rules: files ≤ 300 lines, no `.unwrap()`, all public fns tested.
- `TaskStatus::Stalled` must be backward-compatible with existing display code (add label, grep all `match status`).
- Migration must be idempotent + reversible via DROP TABLE.
