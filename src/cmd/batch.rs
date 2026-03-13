// Handler for `aid batch <file>` — dispatch multiple tasks from a TOML batch file.
// Supports sequential and parallel (background) dispatch modes.

use anyhow::{Context, Result};
use std::{path::Path, sync::Arc};

use crate::batch;
use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
use crate::types::TaskStatus;

pub struct BatchArgs {
    pub file: String,
    pub parallel: bool,
    pub wait: bool,
}

pub async fn run(store: Arc<Store>, args: BatchArgs) -> Result<()> {
    let path = Path::new(&args.file);
    let mut config = batch::parse_batch_file(path)
        .with_context(|| format!("Failed to load batch file {}", path.display()))?;
    let total = config.tasks.len();
    let has_dependencies = config.tasks.iter().any(task_has_dependencies);
    let no_groups_set = config.tasks.iter().all(|t| t.group.is_none());
    if total >= 2 && no_groups_set {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("batch");
        let wg = store.create_workgroup(stem, "Auto-created for batch dispatch")?;
        for task in &mut config.tasks {
            task.group = Some(wg.id.to_string());
        }
        eprintln!("[aid] Auto-created workgroup {} for batch {stem}", wg.id);
    }
    println!("Batch: dispatching {total} task(s) from {}", path.display());
    let task_ids = if has_dependencies && args.parallel {
        dispatch_parallel_with_dependencies(store.clone(), &config.tasks).await?
    } else if has_dependencies {
        dispatch_sequential_with_dependencies(store.clone(), &config.tasks).await?
    } else if args.parallel {
        dispatch_parallel(store.clone(), &config.tasks).await?
    } else {
        dispatch_sequential(store.clone(), &config.tasks).await?
    };
    if args.wait && args.parallel && !has_dependencies && !task_ids.is_empty() {
        crate::cmd::wait::wait_for_task_ids(&store, &task_ids).await?;
    }
    println!("Batch: {total} task(s) dispatched");
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BatchTaskOutcome {
    Done,
    Failed,
    Skipped,
}
struct DispatchedTask {
    index: usize,
    task_id: Option<String>,
}
fn task_to_run_args(task: &batch::BatchTask, background: bool) -> RunArgs {
    RunArgs {
        agent_name: task.agent.clone(),
        prompt: task.prompt.clone(),
        dir: task.dir.clone(),
        output: task.output.clone(),
        model: task.model.clone(),
        worktree: task.worktree.clone(),
        group: task.group.clone(),
        verify: task.verify.clone(),
        retry: 0,
        context: vec![],
        skills: task.skills.clone().unwrap_or_default(),
        background,
        announce: true,
        parent_task_id: None,
        on_done: None,
    }
}
fn task_has_dependencies(task: &batch::BatchTask) -> bool {
    task.depends_on
        .as_ref()
        .is_some_and(|depends_on| !depends_on.is_empty())
}
fn task_label(task: &batch::BatchTask, task_idx: usize) -> String {
    task.name.clone().unwrap_or_else(|| format!("#{task_idx}"))
}
async fn dispatch_parallel(store: Arc<Store>, tasks: &[batch::BatchTask]) -> Result<Vec<String>> {
    let handles: Vec<_> = tasks
        .iter()
        .map(|task| {
            let store = store.clone();
            let run_args = task_to_run_args(task, true);
            tokio::spawn(async move { run::run(store, run_args).await })
        })
        .collect();
    let mut first_err = None;
    let mut task_ids = Vec::new();
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
    Ok(task_ids)
}
async fn dispatch_sequential(store: Arc<Store>, tasks: &[batch::BatchTask]) -> Result<Vec<String>> {
    let mut task_ids = Vec::new();
    for (task_idx, task) in tasks.iter().enumerate() {
        match run::run(store.clone(), task_to_run_args(task, false)).await {
            Ok(task_id) => task_ids.push(task_id.to_string()),
            Err(err) => eprintln!("Batch task failed ({}): {err}", task_label(task, task_idx)),
        }
    }
    Ok(task_ids)
}
async fn dispatch_parallel_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
) -> Result<Vec<String>> {
    let dependencies = batch::dependency_indices(tasks)?;
    let levels = batch::topo_levels(tasks)?;
    let mut outcomes = vec![None; tasks.len()];
    let mut task_ids = Vec::new();
    for (level_idx, level) in levels.iter().enumerate() {
        let ready = select_dispatchable_tasks(tasks, level, &dependencies, &mut outcomes);
        println!(
            "[batch] Level {level_idx}: dispatching {} tasks",
            ready.len()
        );
        if ready.is_empty() {
            continue;
        }

        let dispatches = dispatch_level(store.clone(), tasks, &ready).await?;
        let level_ids: Vec<String> = dispatches
            .iter()
            .filter_map(|dispatch| dispatch.task_id.clone())
            .collect();
        if !level_ids.is_empty() {
            crate::cmd::wait::wait_for_task_ids(&store, &level_ids).await?;
            task_ids.extend(level_ids.iter().cloned());
        }
        record_dispatch_outcomes(&store, &dispatches, &mut outcomes)?;
    }
    Ok(task_ids)
}
async fn dispatch_sequential_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
) -> Result<Vec<String>> {
    let dependencies = batch::dependency_indices(tasks)?;
    let mut outcomes = vec![None; tasks.len()];
    let mut task_ids = Vec::new();
    for (task_idx, task) in tasks.iter().enumerate() {
        if let Some(dep_idx) = failed_dependency(task_idx, &dependencies, &outcomes) {
            log_skipped_task(tasks, task_idx, dep_idx);
            outcomes[task_idx] = Some(BatchTaskOutcome::Skipped);
            continue;
        }
        if let Some(dep_idx) = pending_dependency(task_idx, &dependencies, &outcomes) {
            anyhow::bail!(
                "task {} depends on {} which has not run yet; reorder the batch or use --parallel",
                task_label(task, task_idx),
                task_label(&tasks[dep_idx], dep_idx)
            );
        }
        outcomes[task_idx] = Some(
            match run::run(store.clone(), task_to_run_args(task, false)).await {
                Ok(task_id) => {
                    task_ids.push(task_id.to_string());
                    load_task_outcome(&store, task_id.as_str())?
                }
                Err(err) => {
                    eprintln!("Batch task failed ({}): {err}", task_label(task, task_idx));
                    BatchTaskOutcome::Failed
                }
            },
        );
    }
    Ok(task_ids)
}
async fn dispatch_level(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    task_indices: &[usize],
) -> Result<Vec<DispatchedTask>> {
    let handles: Vec<_> = task_indices
        .iter()
        .map(|&task_idx| {
            let store = store.clone();
            let run_args = task_to_run_args(&tasks[task_idx], true);
            tokio::spawn(async move { (task_idx, run::run(store, run_args).await) })
        })
        .collect();
    let mut dispatches = Vec::with_capacity(task_indices.len());
    for handle in handles {
        let (task_idx, result) = handle.await.context("Batch task join failure")?;
        match result {
            Ok(task_id) => dispatches.push(DispatchedTask {
                index: task_idx,
                task_id: Some(task_id.to_string()),
            }),
            Err(err) => {
                eprintln!(
                    "Batch task failed ({}): {err}",
                    task_label(&tasks[task_idx], task_idx)
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
fn select_dispatchable_tasks(
    tasks: &[batch::BatchTask],
    level: &[usize],
    dependencies: &[Vec<usize>],
    outcomes: &mut [Option<BatchTaskOutcome>],
) -> Vec<usize> {
    let mut ready = Vec::new();
    for &task_idx in level {
        if let Some(dep_idx) = failed_dependency(task_idx, dependencies, outcomes) {
            log_skipped_task(tasks, task_idx, dep_idx);
            outcomes[task_idx] = Some(BatchTaskOutcome::Skipped);
            continue;
        }
        ready.push(task_idx);
    }
    ready
}
fn failed_dependency(
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
fn pending_dependency(
    task_idx: usize,
    dependencies: &[Vec<usize>],
    outcomes: &[Option<BatchTaskOutcome>],
) -> Option<usize> {
    dependencies[task_idx]
        .iter()
        .copied()
        .find(|&dep_idx| outcomes[dep_idx].is_none())
}
fn log_skipped_task(tasks: &[batch::BatchTask], task_idx: usize, dep_idx: usize) {
    eprintln!(
        "[batch] Skipping task {} because dependency {} failed",
        task_label(&tasks[task_idx], task_idx),
        task_label(&tasks[dep_idx], dep_idx)
    );
}
fn record_dispatch_outcomes(
    store: &Arc<Store>,
    dispatches: &[DispatchedTask],
    outcomes: &mut [Option<BatchTaskOutcome>],
) -> Result<()> {
    for dispatch in dispatches {
        outcomes[dispatch.index] = Some(match dispatch.task_id.as_deref() {
            Some(task_id) => load_task_outcome(store, task_id)?,
            None => BatchTaskOutcome::Failed,
        });
    }
    Ok(())
}
fn load_task_outcome(store: &Arc<Store>, task_id: &str) -> Result<BatchTaskOutcome> {
    let Some(task) = store.get_task(task_id)? else {
        anyhow::bail!("batch task not found after dispatch: {task_id}");
    };
    Ok(match task.status {
        TaskStatus::Done => BatchTaskOutcome::Done,
        TaskStatus::Pending | TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Failed => BatchTaskOutcome::Failed,
    })
}
