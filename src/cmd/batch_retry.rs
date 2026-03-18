// Batch retry logic for failed/skipped tasks.
// Exports: retry_failed
// Deps: crate::cmd::run, crate::store::Store, crate::types::Task
use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
use crate::types::{Task, TaskStatus};
use anyhow::Result;
use std::sync::Arc;

pub async fn retry_failed(
    store: Arc<Store>,
    group_id: &str,
    agent_override: Option<&str>,
) -> Result<()> {
    crate::sanitize::validate_workgroup_id(group_id)?;
    let tasks = store.list_tasks_by_group(group_id)?;
    let total = tasks.len();
    let retry_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|task| matches!(task.status, TaskStatus::Failed | TaskStatus::Skipped))
        .collect();
    if retry_tasks.is_empty() {
        println!("No failed tasks in {group_id}");
        return Ok(());
    }
    println!(
        "[batch] Retrying {}/{} failed tasks in {group_id}",
        retry_tasks.len(),
        total
    );
    for task in retry_tasks {
        let run_args = retry_task_to_run_args(&task, group_id, agent_override);
        let _ = run::run(store.clone(), run_args).await?;
    }
    Ok(())
}

pub(crate) fn retry_task_to_run_args(task: &Task, group_id: &str, agent_override: Option<&str>) -> RunArgs {
    let (dir, worktree) = retry_target(task);
    RunArgs {
        agent_name: agent_override
            .map(str::to_string)
            .unwrap_or_else(|| task.agent_display_name().to_string()),
        prompt: task.prompt.clone(),
        repo: task.repo_path.clone(),
        dir,
        output: task.output_path.clone(),
        model: task.model.clone(),
        worktree,
        group: Some(group_id.to_string()),
        verify: task.verify.clone(),
        background: true,
        announce: true,
        parent_task_id: Some(task.id.to_string()),
        read_only: task.read_only,
        budget: task.budget,
        ..Default::default()
    }
}

fn retry_target(task: &Task) -> (Option<String>, Option<String>) {
    match task.worktree_path.as_ref() {
        Some(path) if std::path::Path::new(path).exists() => (Some(path.clone()), None),
        Some(_) => (None, task.worktree_branch.clone()),
        None => (task.repo_path.clone(), task.worktree_branch.clone()),
    }
}

