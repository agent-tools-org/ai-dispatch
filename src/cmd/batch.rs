// Handler for `aid batch <file>` — dispatch multiple tasks from a TOML batch file.
// Supports sequential and parallel (background) dispatch modes.

use anyhow::{Context, Result};
use std::{path::Path, sync::Arc};

use crate::batch;
use crate::cmd::run::{self, RunArgs};
use crate::store::Store;

pub struct BatchArgs {
    pub file: String,
    pub parallel: bool,
    pub wait: bool,
}

pub async fn run(store: Arc<Store>, args: BatchArgs) -> Result<()> {
    let path = Path::new(&args.file);
    let config = batch::parse_batch_file(path)
        .with_context(|| format!("Failed to load batch file {}", path.display()))?;
    let total = config.tasks.len();

    println!("Batch: dispatching {total} task(s) from {}", path.display());
    let mut task_ids = Vec::new();
    if args.parallel {
        let handles: Vec<_> = config
            .tasks
            .iter()
            .map(|task| {
                let store = store.clone();
                let run_args = task_to_run_args(task, true);
                tokio::spawn(async move { run::run(store, run_args).await })
            })
            .collect();
        let mut first_err = None;
        for handle in handles {
            match handle.await.context("Batch task join failure") {
                Ok(Ok(task_id)) => task_ids.push(task_id.to_string()),
                Ok(Err(err)) if first_err.is_none() => first_err = Some(err),
                Err(err) if first_err.is_none() => first_err = Some(err),
                _ => {}
            }
        }
        if let Some(err) = first_err {
            return Err(err);
        }
    } else {
        for task in &config.tasks {
            match run::run(store.clone(), task_to_run_args(task, false)).await {
                Ok(task_id) => task_ids.push(task_id.to_string()),
                Err(err) => {
                eprintln!("Batch task failed ({}): {err}", task.agent);
                }
            }
        }
    }

    if args.wait && !task_ids.is_empty() {
        crate::cmd::wait::wait_for_task_ids(&store, &task_ids).await?;
    }

    println!("Batch: {total} task(s) dispatched");
    Ok(())
}

fn task_to_run_args(task: &batch::BatchTask, background: bool) -> RunArgs {
    RunArgs {
        agent_name: task.agent.clone(),
        prompt: task.prompt.clone(),
        dir: task.dir.clone(),
        output: task.output.clone(),
        model: task.model.clone(),
        worktree: task.worktree.clone(),
        verify: task.verify.clone(),
        retry: 0,
        context: vec![],
        background,
        parent_task_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::task_to_run_args;
    use crate::batch::BatchTask;

    #[test]
    fn passes_verify_through_to_run_args() {
        let task = BatchTask {
            agent: "codex".to_string(),
            prompt: "prompt".to_string(),
            dir: Some(".".to_string()),
            output: None,
            model: None,
            worktree: Some("feat/demo".to_string()),
            verify: Some("auto".to_string()),
        };

        let run_args = task_to_run_args(&task, true);
        assert_eq!(run_args.verify.as_deref(), Some("auto"));
        assert!(run_args.background);
    }
}
