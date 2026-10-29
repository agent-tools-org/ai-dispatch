// PTY worker spawn, monitoring and settlement phases.
// Keeps absolute deadlines in the worker and uses existing task completion and process cleanup.

use super::*;
use crate::runtime::pty::bridge::PtyBridge;
use std::time::Instant;

pub(super) fn spawn_for_task(
    caller_owned: bool,
    cmd: &std::process::Command,
    log_path: &Path,
    store: &Arc<Store>,
    task_id: &TaskId,
    start: Instant,
    control: Option<&PtyRunControl>,
) -> Result<PtyBridge> {
    let spawn = spawn_bridge(cmd);
    let mut bridge = match store.with_running_task(caller_owned, task_id.as_str(), || {
        let bridge = spawn().map_err(|error| {
            if caller_owned { crate::runtime::background::AgentSpawnFailure(error).into() }
            else { error }
        })?;
        if let Some(pid) = bridge.child_pid() {
            crate::runtime::background::publish_agent_pid(caller_owned, task_id.as_str(), pid)?;
        }
        Ok(bridge)
    }) {
        Ok(bridge) => bridge,
        Err(err) => {
            super::command::log_spawn_error(log_path, &err);
            fail_task_on_spawn_error(store, task_id, start.elapsed().as_millis() as i64, &err.to_string());
            return Err(err);
        }
    };
    if let Err(error) = crate::runtime::read_only_launch::record(store, task_id, cmd, "exec") {
        let _ = bridge.kill_group();
        return Err(error);
    }
    if let Some(pid) = bridge.child_pid() {
        if let Some(control) = control {
            control.set_agent_pid(pid);
        }
    }
    if control.is_some_and(PtyRunControl::is_interrupted) {
        let _ = bridge.kill_group();
    }
    Ok(bridge)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn monitor_process(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    bridge: &mut PtyBridge,
    log_path: &Path,
    streaming: bool,
    timeout_policy: crate::config::timeout_policy::TimeoutPolicy,
    max_task_cost: Option<f64>,
    start: Instant,
) -> Result<MonitorState> {
    let rx = super::spawn_reader_thread(bridge.take_reader()?);
    let mut log_file = std::fs::File::create(log_path)?;
    let workgroup_id = store.get_task(task_id.as_str())?.and_then(|task| task.workgroup_id);
    let mut state = MonitorState::with_policy(streaming, workgroup_id, timeout_policy);
    state.absolute_deadline = start + timeout_policy.max_duration.min(timeout_policy.hard_cap);
    monitor_bridge(
        agent,
        task_id,
        store,
        bridge,
        &rx,
        &mut log_file,
        &mut state,
        Some(timeout_policy.first_token),
        Some(timeout_policy.idle),
        max_task_cost,
    )?;
    Ok(state)
}

fn settle_bridge(bridge: &mut PtyBridge) -> Result<portable_pty::ExitStatus> {
    if bridge.is_alive() {
        let _ = bridge.kill_group();
    } else {
        if let Some(pid) = bridge.child_pid() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
        }
    }
    bridge.wait()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finish_process(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    bridge: &mut PtyBridge,
    output_path: Option<&str>,
    log_path: &Path,
    model: Option<&str>,
    streaming: bool,
    start: Instant,
    state: &mut MonitorState,
) -> Result<()> {
    let exit_status = settle_bridge(bridge)?;
    finalize_output(agent, task_id, store, output_path, log_path, streaming, &exit_status, state)?;
    record_completion(agent, task_id, store, model, start.elapsed().as_millis() as i64, &state.info)
}
