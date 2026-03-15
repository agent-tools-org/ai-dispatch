// Batch validation/helpers used by cmd::batch dispatch.
// Exports: validate_batch_config, rate_limit_precheck, resolve_dependencies, task helpers
// Deps: crate::batch, crate::rate_limit, crate::store::Store, crate::types
use anyhow::Result;
use chrono::Local;
use std::collections::HashSet;
use std::sync::Arc;
use crate::batch;
use crate::rate_limit;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use super::BatchTaskOutcome;
pub(super) fn validate_batch_config(tasks: &[batch::BatchTask]) -> Result<()> { validate_task_agents(tasks)?; rate_limit_precheck(tasks); Ok(()) }
pub(super) fn resolve_dependencies(tasks: &[batch::BatchTask]) -> Result<Vec<Vec<usize>>> { batch::dependency_indices(tasks) }
pub(super) fn task_has_dependencies(task: &batch::BatchTask) -> bool { task.depends_on.as_ref().is_some_and(|depends_on| !depends_on.is_empty()) }
pub(super) fn task_label(task: &batch::BatchTask, task_idx: usize) -> String { task.name.clone().unwrap_or_else(|| format!("#{task_idx}")) }
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
        eprintln!(
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
                eprintln!(
                    "[aid] Task {} will use fallback agent: {}",
                    task_label(task, task_idx),
                    fallback
                );
            }
        }
    }
    eprintln!(
        "[aid] {}/{} task(s) use rate-limited agents",
        rate_limited_tasks,
        tasks.len()
    );
}
pub(super) fn find_ready_tasks(store: &Arc<Store>, tasks: &[batch::BatchTask], dependencies: &[Vec<usize>], started: &[bool], outcomes: &mut [Option<BatchTaskOutcome>]) -> Result<Vec<usize>> {
    let mut ready = Vec::new();
    for task_idx in 0..tasks.len() {
        if started[task_idx] || outcomes[task_idx].is_some() {
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
pub(super) fn failed_dependency(task_idx: usize, dependencies: &[Vec<usize>], outcomes: &[Option<BatchTaskOutcome>]) -> Option<usize> {
    dependencies[task_idx].iter().copied().find(|&dep_idx| matches!(outcomes[dep_idx], Some(BatchTaskOutcome::Failed) | Some(BatchTaskOutcome::Skipped)))
}
pub(super) fn pending_dependency(task_idx: usize, dependencies: &[Vec<usize>], outcomes: &[Option<BatchTaskOutcome>]) -> Option<usize> {
    dependencies[task_idx].iter().copied().find(|&dep_idx| outcomes[dep_idx].is_none())
}
pub(super) fn record_skipped_task(store: &Arc<Store>, tasks: &[batch::BatchTask], task_idx: usize, dep_idx: usize) -> Result<()> {
    let task_id = insert_skipped_task(store, &tasks[task_idx])?;
    eprintln!(
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
        TaskStatus::Pending
        | TaskStatus::Running
        | TaskStatus::AwaitingInput
        | TaskStatus::Failed => BatchTaskOutcome::Failed,
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
            anyhow::ensure!(
                batch::is_valid_agent(fallback),
                "Unknown fallback agent '{}' for task {}",
                fallback,
                task_label(task, task_idx)
            );
        }
    }
    Ok(())
}
fn insert_skipped_task(store: &Arc<Store>, task: &batch::BatchTask) -> Result<TaskId> {
    let task_id = TaskId::generate();
    let now = Local::now();
    let (agent, custom_agent_name) = match AgentKind::parse_str(&task.agent) {
        Some(kind) => (kind, None),
        None => (AgentKind::Custom, Some(task.agent.clone())),
    };
    store.insert_task(&Task {
        id: task_id.clone(),
        agent,
        custom_agent_name,
        prompt: task.prompt.clone(),
        status: TaskStatus::Skipped,
        parent_task_id: None,
        workgroup_id: task.group.clone(),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some(0),
        model: None,
        cost_usd: None,
        verify: task.verify.clone(),
        verify_status: VerifyStatus::Skipped,
        read_only: task.read_only,
        budget: task.budget,
        created_at: now,
        resolved_prompt: None,
        completed_at: Some(now),
    })?;
    Ok(task_id)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;
    use crate::rate_limit;
    use crate::store::Store;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn stub_task(name: &str, depends_on: Option<Vec<&str>>) -> batch::BatchTask {
        batch::BatchTask {
            name: Some(name.to_string()),
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            dir: None,
            output: None,
            model: None,
            worktree: None,
            group: None,
            verify: None,
            max_duration_mins: None,
            context: None,
            skills: None,
            hooks: None,
            depends_on: depends_on.map(|d| d.into_iter().map(String::from).collect()),
            fallback: None,
            read_only: false,
            budget: false,
        }
    }

    #[test]
    fn find_ready_dispatches_when_individual_dep_satisfied() {
        let store = Arc::new(Store::open_memory().unwrap());
        let tasks = vec![stub_task("A", None), stub_task("B", Some(vec!["A"])), stub_task("C", Some(vec!["A"])), stub_task("D", Some(vec!["B", "C"]))];
        let deps = vec![vec![], vec![0], vec![0], vec![1, 2]];
        let mut outcomes: Vec<Option<BatchTaskOutcome>> = vec![None; 4];
        let started = vec![false; 4];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert_eq!(ready, vec![0]);
        let mut outcomes = vec![Some(BatchTaskOutcome::Done), None, None, None];
        let started = vec![true, false, false, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert_eq!(ready, vec![1, 2]);
        let mut outcomes = vec![Some(BatchTaskOutcome::Done), Some(BatchTaskOutcome::Done), None, None];
        let started = vec![true, true, true, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert!(ready.is_empty());
        let mut outcomes = vec![Some(BatchTaskOutcome::Done), Some(BatchTaskOutcome::Done), Some(BatchTaskOutcome::Done), None];
        let started = vec![true, true, true, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert_eq!(ready, vec![3]);
    }

    #[test]
    fn find_ready_skips_tasks_with_failed_deps() {
        let store = Arc::new(Store::open_memory().unwrap());
        let tasks = vec![stub_task("A", None), stub_task("B", Some(vec!["A"]))];
        let deps = vec![vec![], vec![0]];
        let mut outcomes = vec![Some(BatchTaskOutcome::Failed), None];
        let started = vec![true, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert!(ready.is_empty());
        assert_eq!(outcomes[1], Some(BatchTaskOutcome::Skipped));
    }

    #[test]
    fn test_rate_limit_precheck_does_not_panic() {
        let temp = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(crate::paths::aid_dir()).ok();
        rate_limit::mark_rate_limited(
            &AgentKind::Codex,
            "rate limit exceeded; try again at Mar 19th, 2026 2:27 PM.",
        );
        assert!(rate_limit::is_rate_limited(&AgentKind::Codex));
        let tasks = vec![stub_task("first", None), stub_task("second", None)];
        rate_limit_precheck(&tasks);
    }
}
