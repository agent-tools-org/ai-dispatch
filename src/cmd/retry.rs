// Handler for `aid retry <task-id> --feedback "msg"` — re-dispatch with feedback.
// Builds augmented prompt from original task + feedback, reuses worktree if available.

use anyhow::Result;
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::store::Store;

pub struct RetryArgs {
    pub task_id: String,
    pub feedback: String,
}

pub async fn run(store: Arc<Store>, args: RetryArgs) -> Result<()> {
    let task = store
        .get_task(&args.task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", args.task_id))?;

    // Build augmented prompt with feedback
    let augmented_prompt = format!(
        "[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{prompt}",
        feedback = args.feedback,
        prompt = task.prompt,
    );

    // Reuse worktree if it exists
    let worktree = task.worktree_path.as_ref().and_then(|wt| {
        if std::path::Path::new(wt).exists() {
            // Extract branch name from worktree path for display
            task.worktree_branch.clone()
        } else {
            None
        }
    });

    // For retry, if worktree exists we pass it as dir directly (skip re-creating)
    let (dir, worktree_arg) = if let Some(ref wt) = task.worktree_path {
        if std::path::Path::new(wt).exists() {
            (Some(wt.clone()), None)
        } else {
            (None, worktree)
        }
    } else {
        (None, None)
    };

    println!("Retrying {} with feedback: {}", task.id, truncate(&args.feedback, 60));

    run::run(
        store,
        RunArgs {
            agent_name: task.agent.as_str().to_string(),
            prompt: augmented_prompt,
            dir,
            output: task.output_path.clone(),
            model: task.model.clone(),
            worktree: worktree_arg,
            background: false,
            verify: None,
            retry: 0,
            context: vec![],
            parent_task_id: Some(task.id.as_str().to_string()),
        },
    )
    .await?;

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
