// Batch dispatch orchestration (parallel/sequential, deps, fallback).
// Exports: dispatch_parallel, dispatch_sequential, dispatch_parallel_with_dependencies, dispatch_sequential_with_dependencies
// Deps: crate::batch, crate::cmd::run, crate::store::Store, super::{batch_args,batch_helpers,batch_types,batch_validate}
use crate::batch;
use crate::cmd::run;
use crate::store::Store;
use anyhow::{Context, Result};
use std::sync::Arc;
use super::batch_args::task_to_run_args;
use super::batch_helpers::{resolve_hook_targets, trigger_conditional};
use super::batch_types::{BatchDispatchResult, BatchTaskOutcome, CompletedTask, DispatchedTask};
use super::batch_validate::{find_ready_tasks, load_task_outcome, resolve_dependencies, task_label};
pub(crate) async fn dispatch_parallel(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
) -> Result<BatchDispatchResult> {
    let dependencies = vec![Vec::new(); tasks.len()];
    let default_max = crate::system_resources::recommended_max_concurrent().min(tasks.len());
    let max_active = max_concurrent.unwrap_or(default_max).max(1);
    dispatch_with_dependencies(store, tasks, &dependencies, max_active, auto_fallback).await
}
pub(crate) async fn dispatch_sequential(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    auto_fallback: bool,
) -> Result<BatchDispatchResult> {
    let dependencies = vec![Vec::new(); tasks.len()];
    dispatch_with_dependencies(store, tasks, &dependencies, 1, auto_fallback).await
}
pub(crate) async fn dispatch_parallel_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
) -> Result<BatchDispatchResult> {
    let dependencies = resolve_dependencies(tasks)?;
    let default_max = crate::system_resources::recommended_max_concurrent().min(tasks.len());
    let max_active = max_concurrent.unwrap_or(default_max).max(1);
    dispatch_with_dependencies(store, tasks, &dependencies, max_active, auto_fallback).await
}
pub(crate) async fn dispatch_sequential_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    auto_fallback: bool,
) -> Result<BatchDispatchResult> {
    let dependencies = resolve_dependencies(tasks)?;
    dispatch_with_dependencies(store, tasks, &dependencies, 1, auto_fallback).await
}
async fn dispatch_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    dependencies: &[Vec<usize>],
    max_active: usize,
    auto_fallback: bool,
) -> Result<BatchDispatchResult> {
    if tasks.is_empty() {
        return Ok(BatchDispatchResult {
            task_ids: Vec::new(),
            outcomes: Vec::new(),
        });
    }
    // Pre-create all tasks with Waiting status so they're visible in TUI immediately
    let waiting_ids: Vec<String> = tasks
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let id = task
                .id
                .as_ref()
                .map(|s| crate::types::TaskId(s.clone()))
                .unwrap_or_else(crate::types::TaskId::generate);
            let agent = if task.agent.is_empty() { "auto" } else { &task.agent };
            let prompt_preview = if task.prompt.len() > 120 {
                &task.prompt[..120]
            } else {
                &task.prompt
            };
            if let Err(e) = store.insert_waiting_task(id.as_str(), agent, prompt_preview, task.group.as_deref()) {
                eprintln!("[aid] Warning: failed to pre-create task {i}: {e}");
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
    while outcomes.iter().any(Option::is_none) {
        let ready = find_ready_tasks(
            &store,
            tasks,
            dependencies,
            &started,
            &mut outcomes,
            &triggered,
        )?;
        let available = max_active.saturating_sub(active.len());
        if available > 0 && !ready.is_empty() {
            let dispatch_group: Vec<_> = ready.into_iter().take(available).collect();
            for dispatch in dispatch_level_with_ids(store.clone(), tasks, &dispatch_group, &waiting_ids).await? {
                started[dispatch.index] = true;
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
        if active.is_empty() {
            break;
        }
        for completed in wait_for_any_completion(&store, &mut active)? {
            if let Some(retry_task_id) = maybe_dispatch_auto_fallback(
                store.clone(),
                tasks,
                completed.index,
                &completed.task_id,
                completed.outcome,
                auto_fallback,
                &mut retried,
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

async fn dispatch_level_with_ids(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    task_indices: &[usize],
    waiting_ids: &[String],
) -> Result<Vec<DispatchedTask>> {
    let handles: Vec<_> = task_indices
        .iter()
        .map(|&task_idx| {
            let store = store.clone();
            let siblings: Vec<_> = tasks
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != task_idx)
                .map(|(_, task)| task)
                .collect();
            let mut run_args = task_to_run_args(&tasks[task_idx], &siblings, true, &store);
            run_args.existing_task_id = Some(crate::types::TaskId(waiting_ids[task_idx].clone()));
            tokio::spawn(async move { (task_idx, run::run(store, run_args).await) })
        })
        .collect();
    let mut dispatches = Vec::with_capacity(task_indices.len());
    for handle in handles {
        let (task_idx, result) = handle.await.context("Batch task join failure")?;
        match result {
            Ok(task_id) => dispatches.push(DispatchedTask {
                index: task_idx,
                task_id: Some(task_id.to_string()),
            }),
            Err(err) => {
                eprintln!(
                    "Batch task failed ({}): {err}",
                    task_label(&tasks[task_idx], task_idx)
                );
                dispatches.push(DispatchedTask {
                    index: task_idx,
                    task_id: None,
                });
            }
        }
    }
    Ok(dispatches)
}

fn wait_for_any_completion(store: &Arc<Store>, active: &mut Vec<(usize, String)>) -> Result<Vec<CompletedTask>> {
    loop {
        let mut completed = Vec::new();
        for (i, (_, task_id)) in active.iter().enumerate() {
            if let Some(task) = store.get_task(task_id)?
                && task.status.is_terminal()
            {
                completed.push(i);
            }
        }
        if !completed.is_empty() {
            let mut completed_tasks = Vec::with_capacity(completed.len());
            for &i in completed.iter().rev() {
                let (task_idx, task_id) = active.remove(i);
                completed_tasks.push(CompletedTask {
                    index: task_idx,
                    outcome: load_task_outcome(store, &task_id)?,
                    task_id,
                });
            }
            return Ok(completed_tasks);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
async fn maybe_dispatch_auto_fallback(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    task_idx: usize,
    task_id: &str,
    outcome: BatchTaskOutcome,
    auto_fallback: bool,
    retried: &mut [bool],
) -> Result<Option<String>> {
    if !should_auto_fallback(auto_fallback, retried[task_idx], outcome) {
        return Ok(None);
    }
    let Some((original_agent, fallback_agent)) = auto_fallback_agent(&store, task_id)? else {
        return Ok(None);
    };
    let siblings: Vec<_> = tasks
        .iter()
        .enumerate()
        .filter(|(idx, _)| *idx != task_idx)
        .map(|(_, task)| task)
        .collect();
    let mut run_args = task_to_run_args(&tasks[task_idx], &siblings, true, &store);
    run_args.agent_name = fallback_agent.as_str().to_string();
    run_args.parent_task_id = Some(task_id.to_string());
    retried[task_idx] = true;
    eprintln!(
        "[batch] Auto-fallback: {} -> {} for task {}",
        original_agent,
        fallback_agent.as_str(),
        task_label(&tasks[task_idx], task_idx),
    );
    let retry_id = run::run(store, run_args).await?;
    Ok(Some(retry_id.to_string()))
}
pub(crate) fn should_auto_fallback(
    auto_fallback: bool,
    already_retried: bool,
    outcome: BatchTaskOutcome,
) -> bool {
    auto_fallback && !already_retried && outcome == BatchTaskOutcome::Failed
}
pub(crate) fn auto_fallback_agent(
    store: &Store,
    task_id: &str,
) -> Result<Option<(String, crate::types::AgentKind)>> {
    let Some(task) = store.get_task(task_id)? else {
        anyhow::bail!("batch task not found after dispatch: {task_id}");
    };
    Ok(crate::agent::selection::coding_fallback_for(&task.agent)
        .map(|fallback| (task.agent.as_str().to_string(), fallback)))
}
