// Background task reaper and stale-state cleanup.
// Exports zombie checks, pending timeout cleanup, and failure recording.
// Deps: background process/spec/orphan helpers, Store, task event types.

use anyhow::Result;
use chrono::Local;
use super::orphan;
use super::kill::terminate_task_processes;
use super::spec::{load_spec_for_reaper, BackgroundRunSpec};
use super::waiting;
use super::MAX_WORKERS;
use crate::{config, task::notify, support::paths, support::sanitize};
use crate::store::Store;
use crate::types::{EventKind, PendingReason, Task, TaskEvent, TaskFilter, TaskId, TaskStatus};

pub(crate) const ZOMBIE_FAILURE_DETAIL: &str = "worker disappeared; cause unknown";
const PENDING_TASK_TIMEOUT_SECS: i64 = 600;
const MAX_RUN_HOURS: i64 = crate::config::timeout_policy::DEFAULT_HARD_CAP_HOURS;

pub(super) fn record_worker_failure_skip_notify(store: &Store, task_id: &str, err: &anyhow::Error) -> Result<bool> {
    record_failure_skip_notify(store, task_id, &format!("{err:#}"), &format!("Background worker failed: {err}"))
}

pub(crate) fn check_zombie_tasks_with<F>(store: &Store, is_worker_alive: F) -> Result<Vec<String>>
where
    F: Fn(u32) -> bool,
{
    let config = config::load_config()?;
    let mut cleaned = cleanup_stale_pending_tasks(store)?;
    cleaned.extend(waiting::cleanup_stale_waiting_tasks(
        store,
        config.background.max_task_duration_mins,
    )?);
    let running_tasks = store.list_tasks(TaskFilter::Running)?;
    cleaned.extend(orphan::cleanup_orphaned_idle_tasks(
        store,
        &running_tasks,
        &cleaned,
        &is_worker_alive,
    )?);
    cleanup_running_tasks(store, &running_tasks, &mut cleaned, &is_worker_alive, config.background.max_task_duration_mins)?;
    cleanup_old_running_tasks(store, running_tasks, &mut cleaned)?;
    Ok(cleaned)
}

fn cleanup_running_tasks<F>(
    store: &Store,
    running_tasks: &[Task],
    cleaned: &mut Vec<String>,
    is_worker_alive: &F,
    default_max_duration_mins: i64,
) -> Result<()>
where
    F: Fn(u32) -> bool,
{
    for task in running_tasks {
        let task_id = task.id.as_str();
        if cleaned.iter().any(|id| id == task_id) {
            continue;
        }
        let Ok(Some(spec)) = load_spec_for_reaper(task_id) else {
            continue;
        };
        if let Some(worker_pid) = spec.worker_pid {
            cleanup_pid_task(store, task, &spec, worker_pid, cleaned, is_worker_alive, default_max_duration_mins)?;
        } else {
            cleanup_missing_pid_task(store, task, &spec, cleaned)?;
        }
    }
    Ok(())
}

pub(super) fn cleanup_pid_task<F>(
    store: &Store,
    task: &Task,
    spec: &BackgroundRunSpec,
    worker_pid: u32,
    cleaned: &mut Vec<String>,
    is_worker_alive: &F,
    default_max_duration_mins: i64,
) -> Result<()>
where
    F: Fn(u32) -> bool,
{
    let task_id = task.id.as_str();
    if is_worker_alive(worker_pid) {
        if orphan::cleanup_wedged_live_worker(store, task, spec, worker_pid)? {
            cleaned.push(task_id.to_string());
            return Ok(());
        }
        let elapsed_secs = (Local::now() - task.created_at).num_seconds();
        let max_duration_secs = spec
            .max_duration_secs
            .unwrap_or_else(|| {
                spec.max_duration_mins
                    .unwrap_or(default_max_duration_mins)
                    .max(0) as u64
                    * 60
            });
        crate::cmd::steer::wrap_up_before_timeout(
            store, task_id, elapsed_secs.max(0) as u64, max_duration_secs,
        )?;
        if elapsed_secs.max(0) as u64 > max_duration_secs {
            let detail = format!(
                "Task exceeded max duration ({}s > {}s)",
                elapsed_secs, max_duration_secs
            );
            // Kill unconditionally: a concurrently-terminalized row must still get its processes reaped.
            terminate_task_processes(Some(worker_pid), spec);
            if record_failure(store, task_id, &detail, &detail)? {
                cleaned.push(task_id.to_string());
            }
        }
        return Ok(());
    }
    cleanup_dead_worker(store, task_id, spec, Some(worker_pid), cleaned)
}

fn cleanup_missing_pid_task(
    store: &Store,
    task: &Task,
    spec: &BackgroundRunSpec,
    cleaned: &mut Vec<String>,
) -> Result<()> {
    let task_id = task.id.as_str();
    let age_secs = (Local::now() - task.created_at).num_seconds();
    if age_secs < 60 {
        return Ok(());
    }
    cleanup_dead_worker(store, task_id, spec, None, cleaned)
}

fn cleanup_dead_worker(
    store: &Store,
    task_id: &str,
    spec: &BackgroundRunSpec,
    worker_pid: Option<u32>,
    cleaned: &mut Vec<String>,
) -> Result<()> {
    preserve_zombie_changes(store, task_id, spec)?;
    terminate_task_processes(worker_pid, spec);
    let detail = format!("{ZOMBIE_FAILURE_DETAIL}; worker log: {}",
        paths::task_dir(task_id).join("worker.log").display());
    if record_failure(store, task_id, &detail, &detail)? {
        cleaned.push(task_id.to_string());
    }
    Ok(())
}

pub(super) fn preserve_zombie_changes(store: &Store, task_id: &str, spec: &BackgroundRunSpec) -> Result<()> {
    if let Some(task) = store.get_task(task_id)?
        && !task.read_only
        && !spec.audit_report_mode
        && let Some(ref path) = task.worktree_path
        && std::path::Path::new(path).exists()
        && crate::workspace::commit::has_uncommitted_changes(path).unwrap_or(false)
    {
        let _ = crate::workspace::commit::auto_commit(path, task_id, &task.prompt);
        aid_info!("[aid] Preserved uncommitted changes for zombie task {task_id}");
    }
    Ok(())
}

fn cleanup_old_running_tasks(
    store: &Store,
    running_tasks: Vec<Task>,
    cleaned: &mut Vec<String>,
) -> Result<()> {
    for task in running_tasks {
        let task_id = task.id.as_str();
        if cleaned.iter().any(|id| id == task_id) {
            continue;
        }
        let elapsed = (Local::now() - task.created_at).num_hours();
        let Ok(spec) = load_spec_for_reaper(task_id) else { continue; };
        let max_run_hours = spec.as_ref().map(|spec| {
            crate::config::timeout_policy::TimeoutPolicy::from_env(spec.env.as_ref()).hard_cap_hours()
        }).unwrap_or(MAX_RUN_HOURS);
        if elapsed <= max_run_hours {
            continue;
        }
        aid_info!("[aid] Auto-failing stale task {} (running {}h, max {}h)", task.id, elapsed, max_run_hours);
        if let Some(spec) = spec.as_ref() {
            terminate_task_processes(spec.worker_pid, spec);
        }
        if record_failure(store, task_id, &format!("Auto-failed: exceeded {max_run_hours}h maximum runtime"), &format!("Task exceeded maximum runtime ({max_run_hours}h)"))? {
            cleaned.push(task_id.to_string());
        }
    }
    Ok(())
}

pub(crate) fn cleanup_stale_pending_tasks(store: &Store) -> Result<Vec<String>> {
    let now = Local::now();
    let mut cleaned = Vec::new();
    for task in store.list_tasks(TaskFilter::All)? {
        if task.status != TaskStatus::Pending {
            continue;
        }
        let elapsed_secs = (now - task.created_at).num_seconds();
        if elapsed_secs <= PENDING_TASK_TIMEOUT_SECS {
            continue;
        }
        let task_id = task.id.as_str();
        aid_warn!("[aid] Timing out stale pending task {} (pending for {}s)", task_id, elapsed_secs);
        if !fail_stale_pending_task(store, &task, now, elapsed_secs)? {
            continue;
        }
        cleaned.push(task_id.to_string());
    }
    Ok(cleaned)
}

pub(crate) fn fail_stale_pending_task(
    store: &Store,
    task: &Task,
    now: chrono::DateTime<Local>,
    elapsed_secs: i64,
) -> Result<bool> {
    let task_id = task.id.as_str();
    let Some(pending_reason) = infer_pending_reason(store, task)? else {
        return Ok(false);
    };
    let event = TaskEvent::at(
        task.id.clone(),
        EventKind::Error,
        format!("Task timed out in pending state after {}s (reason: {})",
            elapsed_secs, pending_reason),
        now,
    );
    if !store.settle_task_status(task_id, TaskStatus::Failed, Some(&[TaskStatus::Pending]),
        PendingReason::parse_str(&pending_reason), Some(&event))? {
        return Ok(false);
    }
    crate::task::lifecycle::salvage_failed_task(store, &task.id);
    notify_task_completion(store, task_id)?;
    Ok(true)
}

fn infer_pending_reason(store: &Store, task: &Task) -> Result<Option<String>> {
    if let Some(reason) = task.pending_reason.as_ref() {
        return Ok(Some(reason.clone()));
    }
    let Ok(spec) = load_spec_for_reaper(task.id.as_str()) else {
        return Ok(None);
    };
    if crate::routing::rate_limit::is_rate_limited(&task.agent, task.custom_agent_name.as_deref()) {
        return Ok(Some(PendingReason::RateLimited.as_str().to_string()));
    }
    if store.list_tasks(TaskFilter::Running)?.len() >= MAX_WORKERS {
        return Ok(Some(PendingReason::WorkerCapacity.as_str().to_string()));
    }
    let has_agent_pid = spec.and_then(|spec| spec.agent_pid).is_some();
    if has_agent_pid {
        Ok(Some(PendingReason::AgentStarting.as_str().to_string()))
    } else {
        Ok(Some(PendingReason::Unknown.as_str().to_string()))
    }
}

pub(super) fn record_failure_skip_notify(store: &Store, task_id: &str, stderr_detail: &str, event_detail: &str) -> Result<bool> {
    sanitize::validate_task_id(task_id)?;
    if !crate::task::lifecycle::fail_active_execution(store, task_id)? {
        return Ok(false);
    }
    let stderr_path = paths::stderr_path(task_id);
    use std::io::Write;
    writeln!(std::fs::OpenOptions::new().create(true).append(true).open(&stderr_path)?, "{stderr_detail}")?;
    crate::runtime::pty::watch::append_failed_terminal_sentinel(
        &TaskId(task_id.to_string()),
        &paths::log_path(task_id),
        event_detail,
    );
    store.insert_event(&TaskEvent::new(
        TaskId(task_id.to_string()),
        EventKind::Error,
        event_detail.to_string(),
    ))?;
    Ok(true)
}

pub(crate) fn record_failure(store: &Store, task_id: &str, stderr_detail: &str, event_detail: &str) -> Result<bool> {
    let recorded = record_failure_skip_notify(store, task_id, stderr_detail, event_detail)?;
    if recorded {
        notify_task_completion(store, task_id)?;
    }
    Ok(recorded)
}

pub(super) fn notify_task_completion(store: &Store, task_id: &str) -> Result<()> {
    if let Some(task) = store.get_task(task_id)? { notify::notify_completion(&task); }
    Ok(())
}
