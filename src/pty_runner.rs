// PTY-backed agent execution for interactive background tasks.
// Runs combined stdout/stderr through prompt detection and file-based input forwarding.

use anyhow::Result;
use chrono::Local;
use std::io::Read;
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;

use crate::agent::Agent;
use crate::cost;
use crate::pty_bridge::PtyBridge;
use crate::pty_runner_control::PtyRunControl;
use crate::pty_watch::{finalize_output, monitor_bridge, MonitorState};
use crate::store::Store;
use crate::store::TaskCompletionUpdate;
use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};

#[allow(clippy::too_many_arguments)]
pub fn run_agent_process(
    agent: &dyn Agent,
    cmd: &std::process::Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
) -> Result<()> {
    run_agent_process_with_control(
        agent,
        cmd,
        task_id,
        store,
        log_path,
        output_path,
        model,
        streaming,
        crate::timeout_policy::TimeoutPolicy::from_command(cmd),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_agent_process_with_control(
    agent: &dyn Agent,
    cmd: &std::process::Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
    timeout_policy: crate::timeout_policy::TimeoutPolicy,
    control: Option<PtyRunControl>,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut bridge = match spawn_bridge(cmd, log_path) {
        Ok(bridge) => bridge,
        Err(err) => {
            fail_task_on_spawn_error(
                store,
                task_id,
                start.elapsed().as_millis() as i64,
                &err.to_string(),
            );
            return Err(err);
        }
    };
    if let Some(pid) = bridge.child_pid() {
        if let Some(control) = &control {
            control.set_agent_pid(pid);
        }
        let _ = crate::background::update_agent_pid(task_id.as_str(), pid);
    }
    if control.as_ref().is_some_and(PtyRunControl::is_interrupted) {
        let _ = bridge.kill_group();
    }
    let rx = spawn_reader_thread(bridge.take_reader()?);
    let mut log_file = std::fs::File::create(log_path)?;
    let workgroup_id = store.get_task(task_id.as_str())?.and_then(|task| task.workgroup_id);
    let mut state = MonitorState::with_policy(streaming, workgroup_id, timeout_policy);
    monitor_bridge(
        agent,
        task_id,
        store,
        &mut bridge,
        &rx,
        &mut log_file,
        &mut state,
        Some(timeout_policy.first_token),
        Some(timeout_policy.idle),
    )?;
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
    let exit_status = bridge.wait()?;
    finalize_output(
        agent,
        task_id,
        store,
        output_path,
        log_path,
        streaming,
        &exit_status,
        &mut state,
    )?;
    record_completion(
        agent,
        task_id,
        store,
        model,
        start.elapsed().as_millis() as i64,
        &state.info,
    )
}

fn spawn_bridge(cmd: &std::process::Command, log_path: &Path) -> Result<PtyBridge> {
    let (argv, dir, env) = command_parts(cmd);
    match PtyBridge::spawn(&argv, dir.as_deref(), env) {
        Ok(bridge) => Ok(bridge),
        Err(err) => {
            let error_msg = format!("Failed to spawn agent process: {err}");
            aid_error!("[aid] {error_msg}");
            write_spawn_error_log(log_path, &error_msg);
            Err(anyhow::anyhow!(error_msg))
        }
    }
}

fn fail_task_on_spawn_error(
    store: &Arc<Store>,
    task_id: &TaskId,
    duration_ms: i64,
    detail: &str,
) {
    let _ = std::fs::write(crate::paths::stderr_path(task_id.as_str()), format!("{detail}\n"));
    let event = TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: detail.to_string(),
        metadata: None,
    };
    let _ = crate::task_lifecycle::complete_task_atomic(
        store.as_ref(),
        TaskCompletionUpdate {
            id: task_id.as_str(),
            status: TaskStatus::Failed,
            tokens: None,
            duration_ms,
            // Nothing ran, so nothing was observed.
            observed_model: None,
            cost_usd: None,
            exit_code: None,
        },
        &event,
    );
    crate::state::refresh_project_state(store.as_ref(), task_id);
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    rx
}

fn record_completion(
    agent: &dyn Agent,
    task_id: &TaskId,
    store: &Arc<Store>,
    model: Option<&str>,
    duration_ms: i64,
    info: &CompletionInfo,
) -> Result<()> {
    // `info.model` is what the CLI said it ran; `model` is what aid asked for.
    // They are kept apart from here on. Collapsing them with `.or()` is what
    // recorded `gemini-3.6-flash-low` as the model the `claude` CLI ran
    // (`t-bd455a68`, which failed for exactly that reason) and cursor's
    // `composer-2` as an `agy` model (`t-702f7bcb`).
    let observed_model = info.model.as_deref();
    // Costing still falls back to the request, because an estimate is openly an
    // estimate — and with both values now stored, a reader can tell which basis
    // any given row used instead of having to assume.
    let costing_model = observed_model.or(model);
    let cost_usd = info.cost_usd.or_else(|| {
        info.tokens
            .and_then(|tokens| cost::estimate_cost(tokens, costing_model, agent.kind()))
    });
    let event = crate::types::TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: if info.status == crate::types::TaskStatus::Done {
            crate::types::EventKind::Completion
        } else {
            crate::types::EventKind::Error
        },
        detail: format!(
            "{} ({}{}{})",
            info.status.label(),
            format_duration(duration_ms),
            info.tokens
                .map(|t| format!(", {} tokens", t))
                .unwrap_or_default(),
            cost_usd
                .map(|c| format!(", {}", cost::format_cost(Some(c))))
                .unwrap_or_default(),
        ),
        metadata: None,
    };
    crate::task_lifecycle::complete_task_atomic(
        store.as_ref(),
        TaskCompletionUpdate {
            id: task_id.as_str(),
            status: info.status,
            tokens: info.tokens,
            duration_ms,
            observed_model,
            cost_usd,
            exit_code: info.exit_code,
        },
        &event,
    )?;
    crate::state::refresh_project_state(store.as_ref(), task_id);
    println!(
        "Task {} {} ({}{}{})",
        task_id,
        info.status.label(),
        format_duration(duration_ms),
        info.tokens
            .map(|tokens| format!(", {} tokens", tokens))
            .unwrap_or_default(),
        cost_usd
            .map(|cost| format!(", {}", cost::format_cost(Some(cost))))
            .unwrap_or_default(),
    );
    Ok(())
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn command_parts(
    cmd: &std::process::Command,
) -> (Vec<String>, Option<String>, Vec<(String, String)>) {
    let argv = std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let dir = cmd
        .get_current_dir()
        .map(|path| path.to_string_lossy().into_owned());
    let env = cmd
        .get_envs()
        .filter_map(|(key, value)| {
            Some((
                key.to_string_lossy().into_owned(),
                value?.to_string_lossy().into_owned(),
            ))
        })
        .collect();
    (argv, dir, env)
}

fn write_spawn_error_log(log_path: &Path, message: &str) {
    let event = serde_json::json!({
        "type": "error",
        "source": "spawn",
        "message": message,
        "timestamp": Local::now().to_rfc3339(),
    });
    let _ = std::fs::write(log_path, format!("{event}\n"));
}

#[cfg(test)]
#[path = "pty_runner_tests.rs"]
mod tests;
