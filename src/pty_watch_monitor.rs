// PTY monitor helpers moved from the shared monitor.
// Deps: parent monitor state, agent parsing, Store and PTY control.

use super::*;

#[cfg(test)]
#[path = "pty_watch_terminal_events_tests.rs"]
mod terminal_events_tests;

#[allow(clippy::too_many_arguments)]
pub(crate) fn monitor_bridge(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    bridge: &mut PtyBridge,
    rx: &mpsc::Receiver<Vec<u8>>,
    log_file: &mut std::fs::File,
    state: &mut MonitorState,
    first_token_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    max_task_cost: Option<f64>,
) -> Result<()> {
    let mut reader_done = false;
    let mut child_exited_at: Option<Instant> = None;
    let mut decoder = utf8::Utf8Chunks::default();
    let mut last_child_activity = std::time::UNIX_EPOCH;
    while !drained(bridge, reader_done, &mut child_exited_at) {
        if deadline_elapsed(state, store, task_id)? {
            break;
        }
        match rx.recv_timeout(INPUT_POLL_INTERVAL) {
            Ok(bytes) => {
                handle_bytes(state, agent, task_id, store, log_file, &mut decoder, bytes)?;
                if cost_exceeded(state, store, task_id, bridge, max_task_cost) {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if handle_idle_timeout(
                    state,
                    agent,
                    store,
                    task_id,
                    bridge,
                    first_token_timeout,
                    idle_timeout,
                    &mut last_child_activity,
                )? {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                state.handle_chunk(agent, task_id, store, log_file, decoder.flush())?;
                reader_done = true;
            }
        }
        forward_input(state, agent, bridge, store, task_id)?;
    }
    state.flush_trailing_output(agent, task_id, store)
}

fn deadline_elapsed(state: &mut MonitorState, store: &Store, task_id: &TaskId) -> Result<bool> {
    if Instant::now() < state.absolute_deadline {
        return Ok(false);
    }
    state.info.status = TaskStatus::Failed;
    store.insert_agent_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: "Failed during execution: exceeded hard timeout".into(),
        metadata: None,
    })?;
    Ok(true)
}

fn handle_bytes(
    state: &mut MonitorState, agent: &dyn Agent, task_id: &TaskId, store: &Arc<Store>,
    log_file: &mut std::fs::File, decoder: &mut utf8::Utf8Chunks, bytes: Vec<u8>,
) -> Result<()> {
    state.received_raw_bytes = true;
    state.last_raw_chunk_time = Instant::now();
    state.handle_chunk(agent, task_id, store, log_file, decoder.push(bytes))
}

fn refresh_child_activity(
    state: &mut MonitorState,
    task_id: &TaskId,
    bridge: &PtyBridge,
    idle: Option<Duration>,
    last_activity: &mut std::time::SystemTime,
) {
    let saw_output = state.received_raw_bytes
        || crate::paths::agent_has_produced_bytes(task_id.as_str(), state.start_system_time);
    if !saw_output {
        return;
    }
    if let Some(activity) = crate::process_monitor::activity::latest(
        task_id.as_str(),
        bridge.child_pid(),
        idle.unwrap_or(Duration::from_secs(
            crate::timeout_policy::DEFAULT_IDLE_SECS,
        )),
    ) && activity > *last_activity
    {
        *last_activity = activity;
        let age = activity.elapsed().unwrap_or_default();
        if age < state.last_progress_time.elapsed() {
            state.last_progress_time = Instant::now() - age;
            state.idle_warned = false;
            state.idle_nudged = false;
        }
    }
}

fn record_hang(
    state: &mut MonitorState,
    store: &Store,
    task_id: &TaskId,
    timeout: Duration,
    first_token: bool,
) -> Result<()> {
    state.info.status = TaskStatus::Failed;
    process_monitor::insert_hung_detected_events(
        store,
        task_id,
        timeout.as_secs(),
        state.progress_count(),
        state.last_progress_detail().as_deref(),
        first_token,
    )
}

fn cost_exceeded(
    state: &mut MonitorState,
    store: &Store,
    task_id: &TaskId,
    bridge: &mut PtyBridge,
    max: Option<f64>,
) -> bool {
    let Some((cost, max)) = state
        .info
        .cost_usd
        .zip(max)
        .filter(|(cost, max)| cost > max)
    else {
        return false;
    };
    let _ = store.insert_agent_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: format!("Task killed: cost ${cost:.2} exceeded ceiling ${max:.2}"),
        metadata: None,
    });
    state.info.status = TaskStatus::Failed;
    let _ = bridge.kill_group();
    true
}
fn drained(bridge: &mut PtyBridge, reader_done: bool, exited_at: &mut Option<Instant>) -> bool {
    if bridge.is_alive() {
        return false;
    }
    reader_done || exited_at.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(2)
}

#[allow(clippy::too_many_arguments)]
fn handle_idle_timeout(
    state: &mut MonitorState,
    agent: &dyn Agent,
    store: &Arc<Store>,
    task_id: &TaskId,
    bridge: &PtyBridge,
    first_token: Option<Duration>,
    idle: Option<Duration>,
    last_activity: &mut std::time::SystemTime,
) -> Result<bool> {
    state.handle_timeout(store, task_id)?;
    if let Some(timeout) = first_token
        && state.first_token_hang_elapsed(agent.streaming(), timeout, task_id.as_str())
    {
        record_hang(state, store, task_id, timeout, true)?;
        return Ok(true);
    }
    refresh_child_activity(state, task_id, bridge, idle, last_activity);
    if let Some(timeout) = idle
        && state.idle_hang_elapsed(agent.streaming(), timeout, task_id.as_str())
    {
        record_hang(state, store, task_id, timeout, false)?;
        return Ok(true);
    }
    Ok(false)
}
fn forward_input(
    state: &mut MonitorState,
    agent: &dyn Agent,
    bridge: &mut PtyBridge,
    store: &Arc<Store>,
    task_id: &TaskId,
) -> Result<()> {
    let accepts_input = agent.accepts_interactive_input();
    if accepts_input {
        state.maybe_forward_input(bridge, store, task_id)?;
        state.maybe_forward_steer(bridge, store, task_id)?;
        state.maybe_consume_reply(bridge, store, task_id)?;
    }
    state.maybe_handle_idle(store, task_id, accepts_input && agent.accepts_idle_nudge())
}
