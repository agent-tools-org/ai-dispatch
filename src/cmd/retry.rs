// Handler for `aid retry` plus a silent helper that returns the new task id.
// Reuses the original task config and dispatches a child task with feedback.

use anyhow::Result;
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
use crate::types::TaskId;

pub struct RetryArgs {
    pub task_id: String,
    pub feedback: String,
}

pub async fn run(store: Arc<Store>, args: RetryArgs) -> Result<()> {
    let _ = retry_task(store, args, true).await?;
    Ok(())
}

pub async fn retry_task(store: Arc<Store>, args: RetryArgs, announce: bool) -> Result<TaskId> {
    let task = store
        .get_task(&args.task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", args.task_id))?;
    let prompt = format!(
        "[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{prompt}",
        feedback = args.feedback,
        prompt = task.prompt,
    );
    let worktree = reusable_worktree(&task);
    let (dir, worktree_arg) = resolve_retry_target(&task, worktree);

    if announce {
        println!(
            "Retrying {} with feedback: {}",
            task.id,
            truncate(&args.feedback, 60)
        );
    }

    run::run(
        store,
        RunArgs {
            agent_name: task.agent.as_str().to_string(),
            prompt,
            repo: task.repo_path.clone(),
            dir,
            output: task.output_path.clone(),
            model: task.model.clone(),
            worktree: worktree_arg,
            base_branch: None,
            group: task.workgroup_id.clone(),
            background: false,
            announce,
            verify: None,
            max_duration_mins: None,
            retry: 0,
            context: vec![],
            skills: vec![],
            template: None,
            parent_task_id: Some(task.id.as_str().to_string()),
            on_done: None,
            fallback: None,
        },
    )
    .await
}

fn reusable_worktree(task: &crate::types::Task) -> Option<String> {
    task.worktree_path.as_ref().and_then(|path| {
        if std::path::Path::new(path).exists() {
            task.worktree_branch.clone()
        } else {
            None
        }
    })
}

fn resolve_retry_target(
    task: &crate::types::Task,
    worktree: Option<String>,
) -> (Option<String>, Option<String>) {
    match task.worktree_path.as_ref() {
        Some(path) if std::path::Path::new(path).exists() => (Some(path.clone()), None),
        Some(_) => (None, worktree),
        None => (None, None),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
