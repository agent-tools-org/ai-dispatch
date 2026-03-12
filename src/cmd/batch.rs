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
}

pub async fn run(store: Arc<Store>, args: BatchArgs) -> Result<()> {
    let path = Path::new(&args.file);
    let config = batch::parse_batch_file(path)
        .with_context(|| format!("Failed to load batch file {}", path.display()))?;
    let total = config.tasks.len();

    println!("Batch: dispatching {total} task(s) from {}", path.display());
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
                Ok(Ok(())) => {}
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
            if let Err(err) = run::run(store.clone(), task_to_run_args(task, false)).await {
                eprintln!("Batch task failed ({}): {err}", task.agent);
            }
        }
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
        context: vec![],
        background,
    }
}
