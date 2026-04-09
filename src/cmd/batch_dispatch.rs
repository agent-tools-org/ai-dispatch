// Batch dispatch orchestration (parallel/sequential, deps, fallback).
// Exports: dispatch_parallel, dispatch_sequential, dispatch_parallel_with_dependencies, dispatch_sequential_with_dependencies
// Deps: crate::batch, crate::cmd::run, crate::store::Store, super::{batch_args,batch_helpers,batch_types,batch_validate}
use crate::batch;
use crate::store::Store;
use crate::types::Task;
use anyhow::Result;
use std::sync::Arc;
#[path = "batch_dispatch_concurrency.rs"]
mod batch_dispatch_concurrency;
use self::batch_dispatch_concurrency::effective_max_active;
use super::batch_dispatch_support::{
    dispatch_level_with_ids,
    maybe_dispatch_auto_fallback,
    poll_completed_tasks,
};
use super::batch_helpers::{resolve_hook_targets, trigger_conditional};
use super::batch_types::{BatchDispatchResult, BatchTaskOutcome};
use super::batch_wait_timeout::ReadyWaitTracker;
use super::batch_validate::{find_ready_tasks, resolve_dependencies};

pub(crate) async fn dispatch_parallel(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = vec![Vec::new(); tasks.len()];
    dispatch_with_dependencies(store, tasks, &dependencies, max_concurrent, auto_fallback, shared_dir_path).await
}
pub(crate) async fn dispatch_sequential(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = vec![Vec::new(); tasks.len()];
    dispatch_with_dependencies(store, tasks, &dependencies, Some(1), auto_fallback, shared_dir_path).await
}
pub(crate) async fn dispatch_parallel_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = resolve_dependencies(tasks)?;
    dispatch_with_dependencies(store, tasks, &dependencies, max_concurrent, auto_fallback, shared_dir_path).await
}
pub(crate) async fn dispatch_sequential_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = resolve_dependencies(tasks)?;
    dispatch_with_dependencies(store, tasks, &dependencies, Some(1), auto_fallback, shared_dir_path).await
}
async fn dispatch_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    dependencies: &[Vec<usize>],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    if tasks.is_empty() {
        return Ok(BatchDispatchResult {
            task_ids: Vec::new(),
            outcomes: Vec::new(),
        });
    }
    let mut rate_warned = std::collections::HashSet::new();
    for task in tasks {
        let agent_name = if task.agent.is_empty() {
            "codex"
        } else {
            task.agent.as_str()
        };
        let Some(kind) = crate::types::AgentKind::parse_str(agent_name) else {
            continue;
        };
        if !crate::rate_limit::is_rate_limited(&kind) || !rate_warned.insert(agent_name.to_string()) {
            continue;
        }
        if task.fallback.is_some() {
            aid_warn!("[batch] {} is rate-limited — tasks with fallback will auto-cascade", agent_name);
        } else {
            aid_warn!(
                "[batch] {} is rate-limited — tasks without fallback may fail. Consider adding fallback.",
                agent_name
            );
        }
    }
    // Pre-create all tasks with Waiting status so they're visible in TUI immediately
    let waiting_ids: Vec<String> = tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let id = task
                .id
                .as_ref()
                .filter(|s| crate::sanitize::is_valid_task_id(s))
                .map(|s| crate::types::TaskId(s.clone()))
                .unwrap_or_else(crate::types::TaskId::generate);
            let agent = if task.agent.is_empty() { "auto" } else { &task.agent };
            if let Err(e) = store.insert_waiting_task(
                id.as_str(),
                agent,
                &task.prompt,
                None,
                task.group.as_deref(),
                task.dir.as_deref(),
                task.worktree.as_deref(),
                task.model.as_deref(),
                task.verify.as_deref(),
                task.read_only,
                task.budget,
            ) {
                aid_warn!("[aid] Warning: failed to pre-create task {i}: {e}");
            }
            id.to_string()
        })
        .collect();
    let name_map = batch::task_name_map(tasks)?;
    let success_targets = resolve_hook_targets(tasks, &name_map, |task| task.on_success.as_deref())?;
    let failure_targets = resolve_hook_targets(tasks, &name_map, |task| task.on_fail.as_deref())?;
    let mut started = vec![false; tasks.len()];
    let mut outcomes: Vec<Option<BatchTaskOutcome>> = vec![None; tasks.len()];
    let mut retried = vec![false; tasks.len()];
    let mut triggered: Vec<bool> = tasks.iter().map(|task| !task.conditional).collect();
    let mut active: Vec<(usize, String)> = Vec::new();
    let mut task_ids: Vec<String> = Vec::new();
    let mut wait_tracker = ReadyWaitTracker::new(tasks);
    while outcomes.iter().any(Option::is_none) {
        let ready = find_ready_tasks(
            &store,
            tasks,
            dependencies,
            &started,
            &mut outcomes,
            &triggered,
        )?;
        wait_tracker.observe_ready(&ready, chrono::Local::now());
        let max_active = effective_max_active(&store, tasks, &ready, &active, max_concurrent)?;
        let available = max_active.saturating_sub(active.len());
        if available > 0 && !ready.is_empty() {
            let dispatch_group: Vec<_> = ready.into_iter().take(available).collect();
            for dispatch in dispatch_level_with_ids(
                store.clone(),
                tasks,
                &dispatch_group,
                &waiting_ids,
                shared_dir_path,
            )
            .await? {
                started[dispatch.index] = true;
                wait_tracker.clear(dispatch.index);
                match dispatch.task_id {
                    Some(task_id) => {
                        task_ids.push(task_id.clone());
                        active.push((dispatch.index, task_id));
                    }
                    None => {
                        outcomes[dispatch.index] = Some(BatchTaskOutcome::Failed);
                        // Mark waiting placeholder as skipped
                        let _ = store.update_task_status(
                            &waiting_ids[dispatch.index],
                            crate::types::TaskStatus::Skipped,
                        );
                        trigger_conditional(
                            BatchTaskOutcome::Failed,
                            dispatch.index,
                            &mut triggered,
                            &success_targets,
                            &failure_targets,
                        );
                    }
                }
            }
        }
        for timeout_task_id in wait_tracker.fail_expired(
            &store,
            &waiting_ids,
            &started,
            chrono::Local::now(),
        )? {
            let Some(task_idx) = waiting_ids.iter().position(|id| id == &timeout_task_id) else {
                continue;
            };
            aid_progress!("[batch] {} TIMEOUT", timeout_task_id);
            started[task_idx] = true;
            if let Some(retry_task_id) = maybe_dispatch_auto_fallback(
                store.clone(),
                tasks,
                task_idx,
                &timeout_task_id,
                BatchTaskOutcome::Failed,
                auto_fallback,
                &mut retried,
                shared_dir_path,
            )
            .await?
            {
                task_ids.push(retry_task_id.clone());
                active.push((task_idx, retry_task_id));
                continue;
            }
            outcomes[task_idx] = Some(BatchTaskOutcome::Failed);
            trigger_conditional(
                BatchTaskOutcome::Failed,
                task_idx,
                &mut triggered,
                &success_targets,
                &failure_targets,
            );
        }
        if active.is_empty() {
            break;
        }
        let completed_tasks = reconcile_and_poll_completed_tasks(&store, &mut active)?;
        if completed_tasks.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            continue;
        }
        for completed in completed_tasks {
            aid_progress!(
                "[batch] {} {}",
                completed.task_id,
                completion_progress_label(store.as_ref(), &completed.task_id, completed.outcome)?
            );
            if let Some(retry_task_id) = maybe_dispatch_auto_fallback(
                store.clone(),
                tasks,
                completed.index,
                &completed.task_id,
                completed.outcome,
                auto_fallback,
                &mut retried,
                shared_dir_path,
            )
            .await?
            {
                task_ids.push(retry_task_id.clone());
                active.push((completed.index, retry_task_id));
                continue;
            }
            outcomes[completed.index] = Some(completed.outcome);
            trigger_conditional(
                completed.outcome,
                completed.index,
                &mut triggered,
                &success_targets,
                &failure_targets,
            );
        }
    }
    // Mark any remaining waiting tasks as skipped (deps never resolved)
    for (i, outcome) in outcomes.iter().enumerate() {
        if outcome.is_none() {
            let _ = store.update_task_status(&waiting_ids[i], crate::types::TaskStatus::Skipped);
        }
    }
    let mut all_ids = waiting_ids;
    all_ids.extend(task_ids);
    Ok(BatchDispatchResult {
        task_ids: all_ids,
        outcomes: outcomes
            .into_iter()
            .map(|outcome| outcome.unwrap_or(BatchTaskOutcome::Skipped))
            .collect(),
    })
}

pub(super) fn reconcile_and_poll_completed_tasks(
    store: &Arc<Store>,
    active: &mut Vec<(usize, String)>,
) -> Result<Vec<super::batch_types::CompletedTask>> {
    let _ = crate::background::check_zombie_tasks(store.as_ref())?;
    poll_completed_tasks(store, active)
}

fn completion_progress_label(
    store: &Store,
    task_id: &str,
    outcome: BatchTaskOutcome,
) -> Result<String> {
    let task = store.get_task(task_id)?;
    Ok(match outcome {
        BatchTaskOutcome::Done => format!("DONE ({})", progress_duration(task.as_ref())),
        BatchTaskOutcome::Failed => format!("FAIL ({})", failure_reason(task.as_ref())),
        BatchTaskOutcome::Skipped => "SKIP".to_string(),
    })
}

fn progress_duration(task: Option<&Task>) -> String {
    let Some(duration_ms) = task.and_then(|task| task.duration_ms) else {
        return "unknown".to_string();
    };
    if duration_ms < 1_000 {
        return format!("{duration_ms}ms");
    }
    let secs = duration_ms / 1_000;
    let tenths = (duration_ms % 1_000) / 100;
    if tenths == 0 {
        format!("{secs}s")
    } else {
        format!("{secs}.{tenths}s")
    }
}

fn failure_reason(task: Option<&Task>) -> String {
    if let Some(reason) = task.and_then(|task| task.pending_reason.as_deref()) {
        return reason.to_string();
    }
    if let Some(exit_code) = task.and_then(|task| task.exit_code) {
        return format!("exit {exit_code}");
    }
    "failed".to_string()
}
