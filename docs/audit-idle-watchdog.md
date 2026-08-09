## Findings

1. **High — live buffered logs still trigger idle escalation.** `idle_hang_elapsed` recognizes recent log writes, but `maybe_handle_idle` still measures only stale `last_progress_time` (`src/pty_watch.rs:328-332`, `:421-430`). A buffered agent writing `agent.log` can therefore be nudged and marked stalled at the idle ladder’s thresholds even though the terminal reap is suppressed. The new tests exercise only the predicate, not this monitor-loop path.

2. **Medium — sandboxed/containerized buffered agents remain unobservable.** `env_with_agent_log` disables `AID_AGENT_LOG` for sandbox/container runs (`src/agent/mod.rs:113-133`), and Grok/agy add their log flags only when that variable exists. Such agents can remain silent on the PTY and still be killed by the unchanged first-token check (`src/pty_watch.rs:401-407`).

## Open Questions

1. **Streaming semantics: PASS.** The streaming path remains progress-clock-only: after the same `last_progress_time.elapsed() > idle` check, streaming agents return `true` directly (`src/pty_watch.rs:421-425`). A stuck buffered agent is still reaped when its progress clock exceeds idle and no watched file has recent nonzero bytes.

2. **Growth: PASS for frozen logs.** The helper checks nonzero size plus `mtime >= window_start - 2s` (`src/paths.rs:80-92`); it does not compare size or mtime against a remembered prior observation. A log written once and then frozen becomes older than the idle window and is reaped, as covered by `idle_hang_fires_for_buffered_when_log_is_stale`.

3. **Shared mechanism: PASS with a maintenance caveat.** The new path reuses `paths::agent_has_produced_bytes`. `background_orphan::agent_bytes_mtime` is a separate scan of the same files for timestamp calculation (`src/background_orphan.rs:111-129`); future changes to liveness qualification must update both. The idle ladder is also a separate consumer that currently remains unaware of log liveness.

**Overall: FIX.**