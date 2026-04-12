// Batch validation/helpers used by cmd::batch dispatch.
// Exports: validate_batch_config, rate_limit_precheck, resolve_dependencies, task helpers
// Deps: crate::batch, crate::rate_limit, crate::store::Store, crate::types
use super::batch_types::BatchTaskOutcome;
pub(super) use super::batch_analyze::analyze_file_overlap;
use crate::agent::classifier;
use crate::batch;
use crate::rate_limit;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use anyhow::Result;
use chrono::Local;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
pub(super) fn validate_batch_config(
    tasks: &[batch::BatchTask],
    parallel: bool,
    force: bool,
) -> Result<()> {
    validate_task_agents(tasks)?;
    if parallel {
        validate_parallel_dir_isolation(tasks, force)?;
    }
    rate_limit_precheck(tasks);
    Ok(())
}
pub(super) fn resolve_dependencies(tasks: &[batch::BatchTask]) -> Result<Vec<Vec<usize>>> {
    batch::dependency_indices(tasks)
}
pub(super) fn task_has_dependencies(task: &batch::BatchTask) -> bool {
    task.depends_on
        .as_ref()
        .is_some_and(|depends_on| !depends_on.is_empty())
}
pub(super) fn task_label(task: &batch::BatchTask, task_idx: usize) -> String {
    task.name.clone().unwrap_or_else(|| format!("#{task_idx}"))
}
pub(super) fn rate_limit_precheck(tasks: &[batch::BatchTask]) {
    let mut unique_agents = HashSet::new();
    for task in tasks {
        if let Some(kind) = AgentKind::parse_str(&task.agent) {
            unique_agents.insert(kind);
        }
    }
    let mut rate_limited = HashSet::new();
    for agent_kind in unique_agents {
        if !rate_limit::is_rate_limited(&agent_kind) {
            continue;
        }
        let recovery_info = rate_limit::get_rate_limit_info(&agent_kind)
            .and_then(|info| info.recovery_at)
            .map(|time| format!(" (try again at {time})"))
            .unwrap_or_default();
        aid_warn!(
            "[aid] Warning: agent '{}' is rate-limited{}",
            agent_kind.as_str(),
            recovery_info
        );
        rate_limited.insert(agent_kind);
    }
    if rate_limited.is_empty() {
        return;
    }
    let mut rate_limited_tasks = 0;
    for (task_idx, task) in tasks.iter().enumerate() {
        let Some(kind) = AgentKind::parse_str(&task.agent) else {
            continue;
        };
        if rate_limited.contains(&kind) {
            rate_limited_tasks += 1;
            if let Some(ref fallback) = task.fallback {
                aid_info!(
                    "[aid] Task {} will use fallback agent: {}",
                    task_label(task, task_idx),
                    fallback
                );
            }
        }
    }
    aid_info!(
        "[aid] {}/{} task(s) use rate-limited agents",
        rate_limited_tasks,
        tasks.len()
    );
}
pub(super) fn find_ready_tasks(
    store: &Arc<Store>,
    tasks: &[batch::BatchTask],
    dependencies: &[Vec<usize>],
    started: &[bool],
    outcomes: &mut [Option<BatchTaskOutcome>],
    triggered: &[bool],
) -> Result<Vec<usize>> {
    let mut ready = Vec::new();
    for task_idx in 0..tasks.len() {
        if started[task_idx] || outcomes[task_idx].is_some() {
            continue;
        }
        if !triggered[task_idx] {
            continue;
        }
        if let Some(dep_idx) = failed_dependency(task_idx, dependencies, outcomes) {
            record_skipped_task(store, tasks, task_idx, dep_idx)?;
            outcomes[task_idx] = Some(BatchTaskOutcome::Skipped);
            continue;
        }
        if pending_dependency(task_idx, dependencies, outcomes).is_none() {
            ready.push(task_idx);
        }
    }
    Ok(ready)
}
pub(super) fn failed_dependency(
    task_idx: usize,
    dependencies: &[Vec<usize>],
    outcomes: &[Option<BatchTaskOutcome>],
) -> Option<usize> {
    dependencies[task_idx].iter().copied().find(|&dep_idx| {
        matches!(
            outcomes[dep_idx],
            Some(BatchTaskOutcome::Failed) | Some(BatchTaskOutcome::Skipped)
        )
    })
}
pub(super) fn pending_dependency(
    task_idx: usize,
    dependencies: &[Vec<usize>],
    outcomes: &[Option<BatchTaskOutcome>],
) -> Option<usize> {
    dependencies[task_idx]
        .iter()
        .copied()
        .find(|&dep_idx| outcomes[dep_idx].is_none())
}
pub(super) fn record_skipped_task(
    store: &Arc<Store>,
    tasks: &[batch::BatchTask],
    task_idx: usize,
    dep_idx: usize,
) -> Result<()> {
    let task_id = insert_skipped_task(store, &tasks[task_idx])?;
    aid_info!(
        "[batch] Skipping task {} ({}) because dependency {} failed",
        task_label(&tasks[task_idx], task_idx),
        task_id,
        task_label(&tasks[dep_idx], dep_idx)
    );
    Ok(())
}
pub(super) fn load_task_outcome(store: &Arc<Store>, task_id: &str) -> Result<BatchTaskOutcome> {
    let Some(task) = store.get_task(task_id)? else {
        anyhow::bail!("batch task not found after dispatch: {task_id}");
    };
    Ok(match task.status {
        TaskStatus::Done | TaskStatus::Merged => BatchTaskOutcome::Done,
        TaskStatus::Skipped => BatchTaskOutcome::Skipped,
        TaskStatus::Waiting
        | TaskStatus::Pending
        | TaskStatus::Running
        | TaskStatus::AwaitingInput
        | TaskStatus::Failed
        | TaskStatus::Stopped => BatchTaskOutcome::Failed,
    })
}
fn validate_task_agents(tasks: &[batch::BatchTask]) -> Result<()> {
    for (task_idx, task) in tasks.iter().enumerate() {
        anyhow::ensure!(
            batch::is_valid_agent(&task.agent),
            "Unknown agent '{}' for task {}",
            task.agent,
            task_label(task, task_idx)
        );
        if let Some(ref fallback) = task.fallback {
            for agent in fallback.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                anyhow::ensure!(
                    batch::is_valid_agent(agent),
                    "Unknown fallback agent '{}' for task {}",
                    agent,
                    task_label(task, task_idx)
                );
            }
        }
    }
    Ok(())
}
fn validate_parallel_dir_isolation(tasks: &[batch::BatchTask], force: bool) -> Result<()> {
    for conflict in shared_dir_conflicts(tasks) {
        if force {
            aid_warn!("{}", parallel_dir_conflict_warning(&conflict));
            continue;
        }
        anyhow::bail!("{}", parallel_dir_conflict_error(&conflict));
    }
    Ok(())
}
fn shared_dir_conflicts(tasks: &[batch::BatchTask]) -> Vec<SharedDirConflict<'_>> {
    let mut dir_counts = BTreeMap::new();
    for task in tasks {
        if task.read_only || task.worktree.is_some() {
            continue;
        }
        let Some(dir) = task.dir.as_deref() else {
            continue;
        };
        *dir_counts.entry(dir).or_insert(0) += 1;
    }
    dir_counts
        .into_iter()
        .filter_map(|(dir, count)| (count > 1).then_some(SharedDirConflict { dir, count }))
        .collect()
}
fn parallel_dir_conflict_error(conflict: &SharedDirConflict<'_>) -> String {
    format!(
        "Error: {} tasks target '{}' without worktree isolation. This causes git index.lock contention. Add `worktree = \"branch-name\"` to each task, or use `--force` to override.",
        conflict.count, conflict.dir
    )
}
fn parallel_dir_conflict_warning(conflict: &SharedDirConflict<'_>) -> String {
    format!(
        "[aid] Warning: {} tasks target '{}' without worktree isolation. This causes git index.lock contention. Proceeding because --force is set.",
        conflict.count, conflict.dir
    )
}
fn insert_skipped_task(store: &Arc<Store>, task: &batch::BatchTask) -> Result<TaskId> {
    let task_id = TaskId::generate();
    let now = Local::now();
    let normalized = task.prompt.trim().to_lowercase();
    let profile = classifier::classify(
        &task.prompt,
        classifier::count_file_mentions(&normalized),
        task.prompt.chars().count(),
    );
    let (agent, custom_agent_name) = match AgentKind::parse_str(&task.agent) {
        Some(kind) => (kind, None),
        None => (AgentKind::Custom, Some(task.agent.clone())),
    };
    store.insert_task(&Task {
        id: task_id.clone(),
        agent,
        custom_agent_name,
        prompt: task.prompt.clone(),
        resolved_prompt: None,
        category: Some(profile.category.label().to_string()),
        status: TaskStatus::Skipped,
        parent_task_id: None,
        workgroup_id: task.group.clone(),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some(0),
        model: None,
        cost_usd: None,
        exit_code: None,
        verify: task.verify.clone(),
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: task.read_only,
        budget: task.budget,
        audit_verdict: None,
        audit_report_path: None,
        created_at: now,
        completed_at: Some(now),
    })?;
    Ok(task_id)
}

struct SharedDirConflict<'a> {
    dir: &'a str,
    count: usize,
}
#[cfg(test)]
#[path = "batch_validate_tests.rs"]
mod tests;
