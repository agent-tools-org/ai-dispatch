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
    repo_root: Option<&str>,
) -> Result<Vec<DispatchedTask>> {
    let shared_dir_path = shared_dir_path.map(str::to_string);
    let repo_root = repo_root.map(str::to_string);
    let mut prepared = Vec::with_capacity(task_indices.len());
    for &task_idx in task_indices {
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
        run_args.repo_root = repo_root.clone();
        run_args.suppress_nested_repo_warning = true;
        run_args.existing_task_id = Some(crate::types::TaskId(waiting_ids[task_idx].clone()));
        if let Some((fallback_agent, remaining_cascade)) =
            pre_dispatch_fallback_choice(&run_args.agent_name, tasks[task_idx].fallback.as_deref())?
        {
            aid_info!(
                "[batch] {} rate-limited → using fallback: {} for task '{}'",
                run_args.agent_name,
                fallback_agent,
                dispatch_task_ref(&tasks[task_idx], task_idx),
            );
            crate::cmd::run::switch_agent(&mut run_args, fallback_agent);
            run_args.cascade = remaining_cascade;
        }
        let progress_ref = format!(
            "{}: {}",
            run_args.agent_name,
            dispatch_task_ref(&tasks[task_idx], task_idx),
        );
        prepared.push((task_idx, progress_ref, run_args));
    }
    let handles: Vec<_> = prepared
        .into_iter()
        .map(|(task_idx, progress_ref, run_args)| {
            let store = store.clone();
            tokio::spawn(async move { (task_idx, progress_ref, run::run(store, run_args).await) })
        })
        .collect();
    let mut dispatches = Vec::with_capacity(task_indices.len());
    for handle in handles {
        let (task_idx, progress_ref, result) = handle.await.context("Batch task join failure")?;
        match result {
            Ok(task_id) => {
                aid_progress!("[batch] {} dispatched ({})", task_id, progress_ref);
                dispatches.push(DispatchedTask {
                    index: task_idx,
                    task_id: Some(task_id.to_string()),
                });
            }
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

pub(super) async fn maybe_dispatch_auto_fallback(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    task_idx: usize,
    task_id: &str,
    outcome: BatchTaskOutcome,
    auto_fallback: bool,
    retried: &mut [bool],
    shared_dir_path: Option<&str>,
    repo_root: Option<&str>,
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
    run_args.repo_root = repo_root.map(str::to_string);
    run_args.suppress_nested_repo_warning = true;
    crate::cmd::run::switch_agent(&mut run_args, fallback_agent.clone());
    run_args.parent_task_id = Some(task_id.to_string());
    retried[task_idx] = true;
    aid_progress!(
        "[batch] {} fallback {} → {}",
        task_id,
        original_agent,
        fallback_agent,
    );
    aid_info!(
        "[batch] Auto-fallback: {} -> {} for task {}",
        original_agent,
        fallback_agent,
        dispatch_task_ref(&tasks[task_idx], task_idx),
    );
    let retry_id = run::run(store, run_args).await?;
    Ok(Some(retry_id.to_string()))
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
) -> Result<Option<(String, Vec<String>)>> {
    let (agent_kind, custom_name) = rate_limit::resolve_agent(agent_name);
    if !rate_limit::is_rate_limited(&agent_kind, custom_name) {
        return Ok(None);
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
) -> Result<Option<(String, String)>> {
    let Some(task) = store.get_task(task_id)? else {
        anyhow::bail!("batch task not found after dispatch: {task_id}");
    };
    if let Some((fallback_name, _)) = tasks
        .get(task_idx)
        .map(|task_spec| available_fallback_after(task.agent.as_str(), task_spec.fallback.as_deref()))
        .transpose()?
        .flatten()
    {
        return Ok(Some((task.agent.as_str().to_string(), fallback_name)));
    }
    if tasks.get(task_idx).and_then(|task_spec| task_spec.fallback.as_deref()).is_some() {
        return Ok(None);
    }
    Ok(crate::agent::selection::coding_fallback_for(
        &task.agent,
        task.category.as_deref(),
        Some(task.prompt.as_str()),
    )
    .map(|fallback| (task.agent.as_str().to_string(), fallback.as_str().to_string())))
}

/// Resolve cascade names the same way `aid run` does: custom agents are valid
/// targets; an unresolvable name is an error, never silently skipped.
fn available_fallback_after(
    current_agent: &str,
    fallback: Option<&str>,
) -> Result<Option<(String, Vec<String>)>> {
    let Some(fallback) = fallback else {
        return Ok(None);
    };
    let names: Vec<&str> = fallback
        .split(',')
        .map(str::trim)
        .filter(|agent_name| !agent_name.is_empty())
        .collect();
    if names.is_empty() {
        return Ok(None);
    }
    let all: Vec<(AgentKind, String)> = names
        .iter()
        .map(|s| {
            AgentKind::parse_str(s)
                .map(|k| (k, (*s).to_string()))
                .or_else(|| {
                    crate::agent::registry::custom_agent_exists(s)
                        .then(|| (AgentKind::Custom, (*s).to_string()))
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Unknown cascade agent '{s}'. Use `aid config agents` to list available agents."
                    )
                })
        })
        .collect::<Result<_>>()?;
    let start = all
        .iter()
        .position(|(_, n)| n == current_agent)
        .map_or(0, |i| i + 1);
    let selected_idx = all[start..]
        .iter()
        .position(|(kind, name)| {
            let candidate_custom = (*kind == AgentKind::Custom).then_some(name.as_str());
            !rate_limit::is_rate_limited(kind, candidate_custom)
        })
        .map(|offset| start + offset);
    let Some(selected_idx) = selected_idx else {
        return Ok(None);
    };
    Ok(Some((
        all[selected_idx].1.clone(),
        all[selected_idx + 1..]
            .iter()
            .map(|(_, name)| name.clone())
            .collect(),
    )))
}
