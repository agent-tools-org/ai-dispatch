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
    pub agent: Option<String>,
}

pub async fn run(store: Arc<Store>, args: RetryArgs) -> Result<()> {
    let retry_id = retry_task(store, args, true).await?;
    eprintln!("[aid] Watch: aid watch --quiet {}", retry_id);
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

    let agent_name = args.agent.unwrap_or_else(|| task.agent_display_name().to_string());
    let session_id = if task.agent == crate::types::AgentKind::OpenCode {
        task.agent_session_id.clone()
    } else {
        None
    };
    run::run(
        store,
        RunArgs {
            agent_name,
            prompt,
            repo: task.repo_path.clone(),
            dir,
            output: task.output_path.clone(),
            model: task.model.clone(),
            worktree: worktree_arg,
            group: task.workgroup_id.clone(),
            verify: task.verify.clone(),
            announce,
            parent_task_id: Some(task.id.as_str().to_string()),
            read_only: task.read_only,
            budget: task.budget,
            session_id,
            ..Default::default()
        },
    )
    .await
}

fn reusable_worktree(task: &crate::types::Task) -> Option<String> {
    // Always return branch name if the original task used a worktree,
    // even if the worktree was auto-cleaned after failure.
    // The retry will reuse the existing worktree or recreate it.
    if task.worktree_path.is_some() {
        task.worktree_branch.clone()
    } else {
        None
    }
}

fn resolve_retry_target(
    task: &crate::types::Task,
    worktree: Option<String>,
) -> (Option<String>, Option<String>) {
    match task.worktree_path.as_ref() {
        Some(path) if std::path::Path::new(path).exists() => {
            // Worktree still exists — run inside it directly, no need to recreate
            (Some(path.clone()), None)
        }
        Some(_) => {
            // Worktree was cleaned up (e.g. auto-cleanup after failure) —
            // pass branch name so run::run recreates a fresh worktree
            (None, worktree)
        }
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
