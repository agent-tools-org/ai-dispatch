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
use crate::runtime::pty::runner_control::PtyRunControl;
use crate::runtime::pty::watch::{finalize_output, monitor_bridge, MonitorState};
use crate::store::Store;
use crate::store::TaskCompletionUpdate;
use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};

mod command;
use command::spawn_bridge;
mod execution;

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
        crate::config::timeout_policy::TimeoutPolicy::from_command(cmd),
        None,
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
    timeout_policy: crate::config::timeout_policy::TimeoutPolicy,
    max_task_cost: Option<f64>,
    control: Option<PtyRunControl>,
) -> Result<()> {
    let start = std::time::Instant::now();
    if agent.kind() == crate::types::AgentKind::Codex
        && crate::runtime::codex_app_server::available(cmd)
    {
        let group = store.get_task(task_id.as_str())?.and_then(|task| task.workgroup_id);
        let info = crate::runtime::codex_app_server::run(crate::runtime::codex_app_server::Run {
            agent, command:cmd, task:task_id, store, log:log_path, output:output_path,
            group:group.as_deref(), policy:timeout_policy, max_cost:max_task_cost,
            control:control.as_ref(),
        })?;
        return record_completion(agent,task_id,store,model,start.elapsed().as_millis() as i64,&info);
    }
    let mut bridge = execution::spawn_for_task(cmd, log_path, store, task_id, start, control.as_ref())?;
    let mut state = execution::monitor_process(
        agent, task_id, store, &mut bridge, log_path, streaming, timeout_policy, max_task_cost, start,
    )?;
    execution::finish_process(
        agent, task_id, store, &mut bridge, output_path, log_path, model, streaming, start, &mut state,
    )
}

fn fail_task_on_spawn_error(
    store: &Arc<Store>,
    task_id: &TaskId,
    duration_ms: i64,
    detail: &str,
) {
    let _ = std::fs::write(crate::support::paths::stderr_path(task_id.as_str()), format!("{detail}\n"));
    let event = TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: detail.to_string(),
        metadata: None,
    };
    let _ = crate::task::lifecycle::complete_task_atomic(
        store.as_ref(),
        TaskCompletionUpdate {
            id: task_id.as_str(),
            status: TaskStatus::Failed,
            tokens: None,
            duration_ms,
            // Nothing ran, so nothing was observed.
            observed_model: None,
            attribution_source: None,
            cost_usd: None,
            exit_code: None,
        },
        &event,
    );
    crate::store::state::refresh_project_state(store.as_ref(), task_id);
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
    let (observed_model, attribution_source) = crate::agent::codex::grade_completion_observation(
        agent, store.as_ref(), task_id, info, model,
    );
    // Costing still falls back to the request, because an estimate is openly an
    // estimate — and with both values now stored, a reader can tell which basis
    // any given row used instead of having to assume.
    let costing_model = observed_model.as_deref().or(model);
    let cost_usd = cost::completion_cost(store, task_id, info, costing_model, agent.kind())?;
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
            crate::routing::capacity::terminal_label(store, task_id, agent.kind(), info.status),
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
    if !crate::task::lifecycle::complete_task_atomic(
        store.as_ref(),
        TaskCompletionUpdate {
            id: task_id.as_str(),
            status: info.status,
            tokens: info.tokens,
            duration_ms,
            observed_model: observed_model.as_deref(),
            attribution_source,
            cost_usd,
            exit_code: info.exit_code,
        },
        &event,
    )? {
        return Ok(());
    }
    crate::store::state::refresh_project_state(store.as_ref(), task_id);
    println!(
        "Task {} {} ({}{}{})",
        task_id,
        crate::routing::capacity::terminal_label(store, task_id, agent.kind(), info.status),
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

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "runner_cost_tests.rs"]
mod cost_tests;

#[cfg(test)]
#[path = "worker_deadline_tests.rs"]
mod deadline_tests;
