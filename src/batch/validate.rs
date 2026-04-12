// Batch validation helpers for agents, DAGs, and conditional task wiring.
// Exports: validation and dependency helpers used by batch parsing and dispatch.
// Deps: anyhow, std collections/io, and parent `BatchTask`.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};

use super::BatchTask;

pub(super) fn validate_agents(tasks: &[BatchTask]) -> Result<()> {
    for (task_idx, task) in tasks.iter().enumerate() {
        if task.agent.trim().is_empty() {
            if task.team.is_some() {
                continue;
            }
            anyhow::bail!("task {} is missing agent", task_label(task, task_idx));
        }
        if task.agent != "auto" && !is_valid_agent(&task.agent) {
            anyhow::bail!("unknown agent: {}", task.agent);
        }
        if let Some(judge_agent) = task.judge.as_deref()
            && !judge_agent.trim().is_empty()
            && !is_valid_agent(judge_agent)
        {
            anyhow::bail!("unknown judge agent: {}", judge_agent);
        }
    }
    Ok(())
}

pub(super) fn validate_fallback_agents(tasks: &[BatchTask]) -> Result<()> {
    for task in tasks {
        if let Some(fallback) = task.fallback.as_deref() {
            for agent in fallback.split(',').map(str::trim) {
                if !agent.is_empty() && !is_valid_agent(agent) {
                    anyhow::bail!("unknown fallback agent: {}", agent);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn is_valid_agent(agent: &str) -> bool {
    crate::types::AgentKind::parse_str(agent).is_some()
        || crate::agent::registry::custom_agent_exists(agent)
}

pub fn auto_sequence_shared_worktrees(
    tasks: &mut [BatchTask],
    writer: &mut impl Write,
) -> io::Result<()> {
    let mut worktree_users: HashMap<&str, Vec<usize>> = HashMap::new();
    for (idx, task) in tasks.iter().enumerate() {
        if let Some(wt) = task.worktree.as_deref() {
            worktree_users.entry(wt).or_default().push(idx);
        }
    }
    for (wt, indices) in &worktree_users {
        if indices.len() < 2 {
            continue;
        }
        for &idx in indices {
            if tasks[idx].name.is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "task #{idx} shares worktree '{wt}' with {} other task(s) but has no name — \
                         add name = \"...\" so aid can auto-sequence shared worktree access",
                        indices.len() - 1
                    ),
                ));
            }
        }
    }

    let mut last_task_by_worktree: HashMap<String, String> = HashMap::new();
    for (task_idx, task) in tasks.iter_mut().enumerate() {
        let Some(worktree) = task.worktree.as_ref() else {
            continue;
        };
        let current_label = task_label(task, task_idx);
        if let Some(previous_task) = last_task_by_worktree.get(worktree).cloned() {
            let depends_on = task.depends_on.get_or_insert_with(Vec::new);
            if !depends_on.iter().any(|dependency| dependency == &previous_task) {
                depends_on.push(previous_task.clone());
            }
            writeln!(
                writer,
                "[aid] Warning: task '{}' shares worktree '{}' with '{}'; auto-sequencing execution.",
                current_label,
                worktree,
                previous_task
            )?;
        }
        if let Some(name) = task.name.as_ref() {
            last_task_by_worktree.insert(worktree.clone(), name.clone());
        }
    }
    Ok(())
}

pub(super) fn validate_dag(tasks: &[BatchTask]) -> Result<()> {
    let dependencies = dependency_indices(tasks)?;
    let mut states = vec![VisitState::Pending; tasks.len()];
    for task_idx in 0..tasks.len() {
        visit_task(task_idx, tasks, &dependencies, &mut states)?;
    }
    Ok(())
}

pub(super) fn validate_conditional_hooks(tasks: &[BatchTask]) -> Result<()> {
    let name_to_index = task_name_map(tasks)?;
    for (task_idx, task) in tasks.iter().enumerate() {
        if let Some(target) = task.on_success.as_deref() {
            validate_conditional_hook(target, "on_success", task_idx, tasks, &name_to_index)?;
        }
        if let Some(target) = task.on_fail.as_deref() {
            validate_conditional_hook(target, "on_fail", task_idx, tasks, &name_to_index)?;
        }
    }
    Ok(())
}

pub(crate) fn dependency_indices(tasks: &[BatchTask]) -> Result<Vec<Vec<usize>>> {
    let name_to_index = task_name_map(tasks)?;
    tasks
        .iter()
        .enumerate()
        .map(|(task_idx, task)| resolve_dependencies(task_idx, task, &name_to_index))
        .collect()
}

pub(crate) fn task_name_map(tasks: &[BatchTask]) -> Result<HashMap<&str, usize>> {
    let mut name_to_index = HashMap::new();
    for (task_idx, task) in tasks.iter().enumerate() {
        let Some(name) = task.name.as_deref() else {
            continue;
        };
        let trimmed = name.trim();
        anyhow::ensure!(!trimmed.is_empty(), "task {task_idx} has an empty name");
        if name_to_index.insert(trimmed, task_idx).is_some() {
            anyhow::bail!("duplicate task name: {trimmed}");
        }
    }
    Ok(name_to_index)
}

fn validate_conditional_hook(
    target: &str,
    hook_name: &str,
    task_idx: usize,
    tasks: &[BatchTask],
    name_to_index: &HashMap<&str, usize>,
) -> Result<()> {
    let trimmed = target.trim();
    anyhow::ensure!(
        !trimmed.is_empty(),
        "task {} has empty {} reference",
        task_label(&tasks[task_idx], task_idx),
        hook_name
    );
    let Some(&target_idx) = name_to_index.get(trimmed) else {
        anyhow::bail!(
            "task {} references unknown task '{}' via {}",
            task_label(&tasks[task_idx], task_idx),
            trimmed,
            hook_name
        );
    };
    if !tasks[target_idx].conditional {
        anyhow::bail!(
            "task {} references {} via {} but target is not conditional",
            task_label(&tasks[task_idx], task_idx),
            task_label(&tasks[target_idx], target_idx),
            hook_name
        );
    }
    Ok(())
}

fn resolve_dependencies(
    task_idx: usize,
    task: &BatchTask,
    name_to_index: &HashMap<&str, usize>,
) -> Result<Vec<usize>> {
    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    if let Some(depends_on) = task.depends_on.as_ref() {
        for dependency_name in depends_on {
            let trimmed = dependency_name.trim();
            anyhow::ensure!(
                !trimmed.is_empty(),
                "task {} has an empty dependency reference",
                task_label(task, task_idx)
            );
            let Some(&dependency_idx) = name_to_index.get(trimmed) else {
                anyhow::bail!(
                    "task {} depends on unknown task: {}",
                    task_label(task, task_idx),
                    trimmed
                );
            };
            if seen.insert(dependency_idx) {
                resolved.push(dependency_idx);
            }
        }
    }
    if let Some(context_from) = task.context_from.as_ref() {
        for source_name in context_from {
            let trimmed = source_name.trim();
            if let Some(&source_idx) = name_to_index.get(trimmed)
                && seen.insert(source_idx)
            {
                resolved.push(source_idx);
            }
        }
    }
    Ok(resolved)
}

fn task_label(task: &BatchTask, task_idx: usize) -> String {
    task.name.clone().unwrap_or_else(|| format!("#{task_idx}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Pending,
    Visiting,
    Visited,
}

fn visit_task(
    task_idx: usize,
    tasks: &[BatchTask],
    dependencies: &[Vec<usize>],
    states: &mut [VisitState],
) -> Result<()> {
    match states[task_idx] {
        VisitState::Visited => return Ok(()),
        VisitState::Visiting => {
            anyhow::bail!(
                "dependency cycle detected at task {}",
                task_label(&tasks[task_idx], task_idx)
            )
        }
        VisitState::Pending => {}
    }
    states[task_idx] = VisitState::Visiting;
    for &dependency_idx in &dependencies[task_idx] {
        visit_task(dependency_idx, tasks, dependencies, states)?;
    }
    states[task_idx] = VisitState::Visited;
    Ok(())
}
