// Batch dispatch helpers for spawning, completion polling, and fallback selection.
// Exports: dispatch_level_with_ids, poll_completed_tasks, pre_dispatch_fallback_choice, should_auto_fallback, auto_fallback_agent, dispatch_task_ref
// Deps: crate::batch, crate::cmd::run, crate::rate_limit, crate::store::Store, super::batch_args, super::batch_types, super::batch_validate
use crate::batch;
use crate::cmd::run;
use crate::rate_limit;
use crate::store::Store;
use crate::types::AgentKind;
use anyhow::{Context, Result};
use std::sync::Arc;

use super::batch_args::task_to_run_args;
use super::batch_types::{BatchTaskOutcome, CompletedTask, DispatchedTask};
use super::batch_validate::load_task_outcome;

pub(super) async fn dispatch_level_with_ids(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    task_indices: &[usize],
    waiting_ids: &[String],
    shared_dir_path: Option<&str>,
) -> Result<Vec<DispatchedTask>> {
    let shared_dir_path = shared_dir_path.map(str::to_string);
    let handles: Vec<_> = task_indices
        .iter()
        .map(|&task_idx| {
            let store = store.clone();
            let shared_dir_path = shared_dir_path.clone();
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
                shared_dir_path.as_deref(),
            );
            run_args.existing_task_id = Some(crate::types::TaskId(waiting_ids[task_idx].clone()));
            if let Some((fallback_agent, remaining_cascade)) =
                pre_dispatch_fallback_choice(&run_args.agent_name, tasks[task_idx].fallback.as_deref())
            {
                aid_info!(
                    "[batch] {} rate-limited → using fallback: {} for task '{}'",
                    run_args.agent_name,
                    fallback_agent.as_str(),
                    dispatch_task_ref(&tasks[task_idx], task_idx),
                );
                run_args.agent_name = fallback_agent.as_str().to_string();
                run_args.cascade = remaining_cascade;
            }
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
                aid_error!(
                    "Batch task failed ({}): {err}",
                    dispatch_task_ref(&tasks[task_idx], task_idx)
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

pub(super) fn poll_completed_tasks(
    store: &Arc<Store>,
    active: &mut Vec<(usize, String)>,
) -> Result<Vec<CompletedTask>> {
    let mut completed = Vec::new();
    for (i, (_, task_id)) in active.iter().enumerate() {
        if let Some(task) = store.get_task(task_id)?
            && task.status.is_terminal()
        {
            completed.push(i);
        }
    }
    let mut completed_tasks = Vec::with_capacity(completed.len());
    for &i in completed.iter().rev() {
        let (task_idx, task_id) = active.remove(i);
        completed_tasks.push(CompletedTask {
            index: task_idx,
            outcome: load_task_outcome(store, &task_id)?,
            task_id,
        });
    }
    Ok(completed_tasks)
}

pub(super) fn dispatch_task_ref(task: &batch::BatchTask, task_idx: usize) -> String {
    task.id
        .as_ref()
        .or(task.name.as_ref())
        .cloned()
        .unwrap_or_else(|| format!("#{task_idx}"))
}

pub(crate) fn pre_dispatch_fallback_choice(
    agent_name: &str,
    fallback: Option<&str>,
) -> Option<(AgentKind, Vec<String>)> {
    let agent_kind = AgentKind::parse_str(agent_name)?;
    if !rate_limit::is_rate_limited(&agent_kind) {
        return None;
    }
    available_fallback_after(agent_name, fallback)
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
    tasks: &[batch::BatchTask],
    task_idx: usize,
) -> Result<Option<(String, AgentKind)>> {
    let Some(task) = store.get_task(task_id)? else {
        anyhow::bail!("batch task not found after dispatch: {task_id}");
    };
    if let Some((fallback_kind, _)) = tasks
        .get(task_idx)
        .and_then(|task_spec| available_fallback_after(task.agent.as_str(), task_spec.fallback.as_deref()))
    {
        return Ok(Some((task.agent.as_str().to_string(), fallback_kind)));
    }
    if tasks.get(task_idx).and_then(|task_spec| task_spec.fallback.as_deref()).is_some() {
        return Ok(None);
    }
    Ok(crate::agent::selection::coding_fallback_for(&task.agent)
        .map(|fallback| (task.agent.as_str().to_string(), fallback)))
}

fn available_fallback_after(
    current_agent: &str,
    fallback: Option<&str>,
) -> Option<(AgentKind, Vec<String>)> {
    let fallback_agents: Vec<_> = fallback?
        .split(',')
        .map(str::trim)
        .filter(|agent_name| !agent_name.is_empty())
        .filter_map(AgentKind::parse_str)
        .collect();
    let start = AgentKind::parse_str(current_agent)
        .and_then(|agent| fallback_agents.iter().position(|candidate| *candidate == agent))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let selected_idx = fallback_agents[start..]
        .iter()
        .position(|candidate| !rate_limit::is_rate_limited(candidate))
        .map(|offset| start + offset)?;
    Some((
        fallback_agents[selected_idx],
        fallback_agents[selected_idx + 1..]
            .iter()
            .map(|agent| agent.as_str().to_string())
            .collect(),
    ))
}
