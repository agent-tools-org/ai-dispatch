// Batch dispatch orchestration (parallel/sequential, deps, fallback).
// Exports: dispatch_parallel, dispatch_sequential, dispatch_parallel_with_dependencies, dispatch_sequential_with_dependencies
// Deps: crate::batch, crate::cmd::run, crate::store::Store, super::{batch_args,batch_helpers,batch_types,batch_validate}
use crate::batch;
use crate::cmd::run;
use crate::store::Store;
use anyhow::Result;
use std::sync::Arc;
use super::batch_args::task_to_run_args;
use super::batch_dispatch_support::{
    auto_fallback_agent,
    dispatch_level_with_ids,
    poll_completed_tasks,
    should_auto_fallback,
};
use super::batch_helpers::{resolve_hook_targets, trigger_conditional};
use super::batch_types::{BatchDispatchResult, BatchTaskOutcome};
use super::batch_wait_timeout::ReadyWaitTracker;
use super::batch_validate::{find_ready_tasks, resolve_dependencies, task_label};

pub(crate) async fn dispatch_parallel(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = vec![Vec::new(); tasks.len()];
    let default_max = crate::system_resources::recommended_max_concurrent().min(tasks.len());
    let max_active = max_concurrent.unwrap_or(default_max).max(1);
    dispatch_with_dependencies(
        store,
        tasks,
        &dependencies,
        max_active,
        auto_fallback,
        shared_dir_path,
    )
    .await
}
pub(crate) async fn dispatch_sequential(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = vec![Vec::new(); tasks.len()];
    dispatch_with_dependencies(store, tasks, &dependencies, 1, auto_fallback, shared_dir_path).await
}
pub(crate) async fn dispatch_parallel_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = resolve_dependencies(tasks)?;
    let default_max = crate::system_resources::recommended_max_concurrent().min(tasks.len());
    let max_active = max_concurrent.unwrap_or(default_max).max(1);
    dispatch_with_dependencies(
        store,
        tasks,
        &dependencies,
        max_active,
        auto_fallback,
        shared_dir_path,
    )
    .await
}
pub(crate) async fn dispatch_sequential_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
) -> Result<BatchDispatchResult> {
    let dependencies = resolve_dependencies(tasks)?;
    dispatch_with_dependencies(store, tasks, &dependencies, 1, auto_fallback, shared_dir_path).await
}
async fn dispatch_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    dependencies: &[Vec<usize>],
    max_active: usize,
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
    let max_active = max_active.max(1);
    let default_max_wait_mins = default_max_wait_mins();
    let mut wait_tracker = ReadyWaitTracker::new(tasks, default_max_wait_mins);
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
        let completed_tasks = poll_completed_tasks(&store, &mut active)?;
        if completed_tasks.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            continue;
        }
        for completed in completed_tasks {
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

fn default_max_wait_mins() -> Option<u64> {
    crate::config::load_config()
        .ok()
        .and_then(|config| u64::try_from(config.background.max_task_duration_mins).ok())
        .filter(|mins| *mins > 0)
}

async fn maybe_dispatch_auto_fallback(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    task_idx: usize,
    task_id: &str,
    outcome: BatchTaskOutcome,
    auto_fallback: bool,
    retried: &mut [bool],
    shared_dir_path: Option<&str>,
) -> Result<Option<String>> {
    if !should_auto_fallback(auto_fallback, retried[task_idx], outcome) {
        return Ok(None);
    }
    let Some((original_agent, fallback_agent)) = auto_fallback_agent(&store, task_id, tasks, task_idx)? else {
        return Ok(None);
    };
    let siblings: Vec<_> = tasks
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx != task_idx)
        .map(|(_, task)| task)
        .collect();
    let mut run_args = task_to_run_args(
        &tasks[task_idx],
        &siblings,
        true,
        &store,
        shared_dir_path,
    );
    run_args.agent_name = fallback_agent.as_str().to_string();
    run_args.parent_task_id = Some(task_id.to_string());
    retried[task_idx] = true;
    aid_info!(
        "[batch] Auto-fallback: {} -> {} for task {}",
        original_agent,
        fallback_agent.as_str(),
        task_label(&tasks[task_idx], task_idx),
    );
    let retry_id = run::run(store, run_args).await?;
    Ok(Some(retry_id.to_string()))
}
