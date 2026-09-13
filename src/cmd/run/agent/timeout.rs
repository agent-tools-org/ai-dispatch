// Foreground process timeout policy for `aid run`.
// Exports run_agent_process_with_timeout plus small testable timeout helpers.
// Depends on watcher activity events, process-group cleanup, and task store updates.
use anyhow::Result;
use chrono::Local;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;

use crate::process_group::{cleanup_process_group, force_kill_process_group};
use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{CompletionInfo, EventKind, TaskEvent, TaskId, TaskStatus};
use crate::watcher;

use super::{format_duration, run_prompt, spawn_child_with_log, write_streaming_output};

#[cfg(not(test))]
const FOREGROUND_TIMEOUT_CHECK_INTERVAL: Duration = Duration::from_secs(30);
#[cfg(test)]
const FOREGROUND_TIMEOUT_CHECK_INTERVAL: Duration = Duration::from_millis(10);

enum ForegroundRunResult {
    Completed(Result<CompletionInfo>),
    TimedOut,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_agent_process_with_timeout(
    agent: &dyn crate::agent::Agent,
    mut cmd: Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
    workgroup_id: Option<&str>,
    timeout_policy: crate::timeout_policy::TimeoutPolicy,
    max_task_cost: Option<f64>,
) -> Result<()> {
    let timeout_mins = timeout_policy.max_duration_mins();
    let deadline = timeout_policy.max_duration;
    let start = Instant::now();
    let idle_timeout = timeout_policy.idle;
    let failure_context = run_prompt::capture_failure_context(store.as_ref(), task_id, &cmd);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match spawn_child_with_log(&mut cmd, log_path) {
        Ok(child) => child,
        Err(err) => {
            let err = err.context("Failed to spawn agent process");
            let stderr = run_prompt::stderr_excerpt(task_id)
                .or_else(|| Some("unavailable (process did not start)".to_string()));
            run_prompt::fail_task_on_agent_spawn(store.as_ref(), task_id, &err, stderr.as_deref());
            return Err(err);
        }
    };
    let child_pid = child.id();
    if let Some(pid) = child_pid {
        let _ = crate::background::update_agent_pid(task_id.as_str(), pid);
    }
    let result = {
        let watch_future = async {
            let info = if streaming {
                watcher::watch_streaming(
                    agent,
                    &mut child,
                    task_id,
                    store,
                    log_path,
                    workgroup_id,
                    idle_timeout,
                    max_task_cost,
                )
                    .await?
            } else {
                let output_path = output_path.map(Path::new);
                watcher::watch_buffered(
                    agent,
                    &mut child,
                    task_id,
                    store,
                    log_path,
                    output_path,
                    workgroup_id,
                )
                .await?
            };
            Ok::<CompletionInfo, anyhow::Error>(info)
        };
        let timeout_future = wait_for_activity_aware_timeout(
            store,
            task_id,
            deadline,
            idle_timeout,
            FOREGROUND_TIMEOUT_CHECK_INTERVAL,
        );
        tokio::pin!(watch_future);
        tokio::pin!(timeout_future);
        tokio::select! {
            result = &mut watch_future => ForegroundRunResult::Completed(result),
            () = &mut timeout_future => {
                if let Some(pid) = child_pid {
                    crate::background::sigkill_process(pid);
                }
                ForegroundRunResult::TimedOut
            }
        }
    };
    let timed_out = matches!(result, ForegroundRunResult::TimedOut);
    if timed_out {
        force_kill_process_group(&child);
    } else {
        cleanup_process_group(&child);
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
    match result {
        ForegroundRunResult::Completed(Ok(info)) => {
            handle_success(
                agent,
                store,
                task_id,
                log_path,
                output_path,
                model,
                streaming,
                start,
                info,
                &failure_context,
            )
        }
        ForegroundRunResult::Completed(Err(err)) => {
            let stderr = run_prompt::stderr_excerpt(task_id);
            run_prompt::insert_phase_error_event(
                store.as_ref(),
                task_id,
                "execution",
                &err.to_string(),
                stderr.as_deref(),
            );
            Err(err)
        }
        ForegroundRunResult::TimedOut => {
            handle_timeout(store, task_id, start, timeout_mins, idle_timeout)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_success(
    agent: &dyn crate::agent::Agent,
    store: &Arc<Store>,
    task_id: &TaskId,
    log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
    start: Instant,
    info: CompletionInfo,
    failure_context: &run_prompt::FailureContext,
) -> Result<()> {
    if let Some(out_path) = output_path {
        let out_path = Path::new(out_path);
        if streaming {
            write_streaming_output(log_path, out_path);
        }
        run_prompt::fill_empty_output_from_log(
            log_path,
            Some(out_path),
            Some(agent.rate_limit_name().unwrap_or_else(|| agent.kind().as_str())),
        )?;
        run_prompt::clean_output_if_jsonl(out_path)?;
    }
    let duration_ms = start.elapsed().as_millis() as i64;
    let exit_code = run_prompt::resolve_failure_exit_code(store.as_ref(), task_id, info.exit_code);
    if info.status == TaskStatus::Failed {
        run_prompt::record_execution_failure(
            store.as_ref(),
            task_id,
            duration_ms,
            exit_code,
            failure_context,
        );
    }
    // Same split as pty_runner::record_completion: the observation stands alone,
    // only the cost estimate may fall back to what was requested.
    let (observed_model, attribution_source) = crate::agent::codex::grade_completion_observation(
        agent, store.as_ref(), task_id, &info, model,
    );
    let costing_model = observed_model.as_deref().or(model);
    let cost_usd = info.cost_usd.or_else(|| {
        info.tokens
            .and_then(|tokens| crate::cost::estimate_cost(tokens, costing_model, agent.kind()))
    });
    crate::task_lifecycle::update_task_completion(store.as_ref(), TaskCompletionUpdate {
        id: task_id.as_str(),
        status: info.status,
        tokens: info.tokens,
        duration_ms,
        observed_model: observed_model.as_deref(),
        attribution_source,
        cost_usd,
        exit_code,
    })?;
    crate::state::refresh_project_state(store.as_ref(), task_id);
    let duration_str = format_duration(duration_ms);
    let tokens_str = info.tokens.map(|t| format!(", {t} tokens")).unwrap_or_default();
    let cost_str = if cost_usd.is_some() {
        format!(", {}", crate::cost::format_cost(cost_usd))
    } else {
        String::new()
    };
    println!("Task {} {} ({}{}{})", task_id, info.status.label(), duration_str, tokens_str, cost_str);
    Ok(())
}

fn handle_timeout(
    store: &Arc<Store>,
    task_id: &TaskId,
    start: Instant,
    timeout_mins: i64,
    idle_timeout: Duration,
) -> Result<()> {
    let duration_ms = start.elapsed().as_millis() as i64;
    crate::task_lifecycle::update_task_completion(store.as_ref(), TaskCompletionUpdate {
        id: task_id.as_str(),
        status: TaskStatus::Failed,
        tokens: None,
        duration_ms,
        // A timeout kills the CLI before it reports anything, so there is no
        // observation to record. The request lives in `model` on the row.
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
    })?;
    crate::state::refresh_project_state(store.as_ref(), task_id);
    let detail = format!(
        "exceeded {timeout_mins}m timeout after {}s without parsed activity",
        idle_timeout.as_secs()
    );
    let event = TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: format!("Failed during execution: {detail}"),
        metadata: None,
    };
    let _ = store.insert_event(&event);
    aid_error!("[aid] {detail}");
    Ok(())
}

async fn wait_for_activity_aware_timeout(
    store: &Arc<Store>,
    task_id: &TaskId,
    max_duration: Duration,
    idle_timeout: Duration,
    check_interval: Duration,
) {
    let start = Instant::now();
    let mut last_activity = start;
    let mut event_count = current_event_count(store, task_id);
    let mut interval = tokio::time::interval(check_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        let current_count = current_event_count(store, task_id);
        if current_count > event_count {
            event_count = current_count;
            last_activity = Instant::now();
        }
        if foreground_timeout_expired(start, last_activity, Instant::now(), max_duration, idle_timeout) {
            return;
        }
    }
}

fn current_event_count(store: &Arc<Store>, task_id: &TaskId) -> usize {
    store
        .get_events(task_id.as_str())
        .map(|events| events.len())
        .unwrap_or_default()
}

fn foreground_timeout_expired(
    start: Instant,
    last_activity: Instant,
    now: Instant,
    max_duration: Duration,
    idle_timeout: Duration,
) -> bool {
    // The max-duration cap is a secondary guard: an active foreground run may
    // continue past it, and is killed only after parsed output has also gone idle.
    now.duration_since(start) >= max_duration && now.duration_since(last_activity) >= idle_timeout
}

#[cfg(test)]
#[path = "timeout_tests.rs"]
mod tests;
