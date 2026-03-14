// Batch dispatch command for running tasks from a TOML file.
// Exports: BatchArgs, run()
// Deps: crate::batch, crate::cmd::run, crate::cmd::batch_validate, crate::store::Store
use anyhow::{Context, Result};
use std::{path::Path, sync::Arc};
use crate::batch;
use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
use crate::types::{TaskId, TaskStatus};
#[path = "batch_validate.rs"]
mod batch_validate;
use batch_validate::{failed_dependency, find_ready_tasks, load_task_outcome, pending_dependency, record_skipped_task, resolve_dependencies, task_has_dependencies, task_label, validate_batch_config};
pub struct BatchArgs { pub file: String, pub parallel: bool, pub wait: bool, pub max_concurrent: Option<usize> }
pub async fn run(store: Arc<Store>, args: BatchArgs) -> Result<()> {
    if args.max_concurrent == Some(0) {
        anyhow::bail!("--max-concurrent must be at least 1");
    }
    let path = Path::new(&args.file);
    let mut config = batch::parse_batch_file(path)
        .with_context(|| format!("Failed to load batch file {}", path.display()))?;
    let total = config.tasks.len();
    validate_batch_config(&config.tasks)?;
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
        dispatch_parallel_with_dependencies(store.clone(), &config.tasks, args.max_concurrent).await?
    } else if has_dependencies {
        dispatch_sequential_with_dependencies(store.clone(), &config.tasks).await?
    } else if args.parallel {
        dispatch_parallel(store.clone(), &config.tasks, args.max_concurrent).await?
    } else {
        dispatch_sequential(store.clone(), &config.tasks).await?
    };
    if args.wait && args.parallel && !has_dependencies && !task_ids.is_empty() {
        crate::cmd::wait::wait_for_task_ids(&store, &task_ids, false).await?;
    }
    let archive_dir = crate::paths::aid_dir().join("batches");
    if let Err(e) = std::fs::create_dir_all(&archive_dir) {
        eprintln!("[aid] Failed to create batch archive dir: {e}");
    } else {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("batch");
        let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let dest = archive_dir.join(format!("{timestamp}-{stem}.toml"));
        match std::fs::copy(path, &dest) {
            Ok(_) => eprintln!("[aid] Archived batch to {}", dest.display()),
            Err(e) => eprintln!("[aid] Failed to archive batch: {e}"),
        }
    }
    println!("Batch: {total} task(s) dispatched");
    // Print watch hint for the caller
    let group_id = config.tasks.first().and_then(|t| t.group.as_deref());
    if let Some(gid) = group_id {
        eprintln!("[aid] Watch: aid watch --quiet --group {gid}");
    } else if task_ids.len() == 1 {
        eprintln!("[aid] Watch: aid watch --quiet {}", task_ids[0]);
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BatchTaskOutcome { Done, Failed, Skipped }
struct DispatchedTask { index: usize, task_id: Option<String> }
type BatchJob<T> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>;
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
        max_duration_mins: task.max_duration_mins.map(|value| value as i64),
        context: task.context.clone().unwrap_or_default(),
        skills: task.skills.clone().unwrap_or_default(),
        background,
        announce: true,
        fallback: task.fallback.clone(),
        read_only: task.read_only,
        budget: task.budget,
        ..Default::default()
    }
}
async fn dispatch_parallel(store: Arc<Store>, tasks: &[batch::BatchTask], max_concurrent: Option<usize>) -> Result<Vec<String>> {
    let throttled = max_concurrent.is_some();
    let jobs = tasks
        .iter()
        .map(|task| {
            let store = store.clone();
            let run_args = task_to_run_args(task, true);
            Box::pin(async move {
                let task_id = run::run(store.clone(), run_args).await?;
                if throttled {
                    wait_for_background_completion(&store, task_id.as_str()).await?;
                }
                Ok(task_id)
            }) as BatchJob<_>
        })
        .collect();
    let task_ids: Vec<TaskId> = run_parallel_jobs(jobs, max_concurrent).await?;
    Ok(task_ids.into_iter().map(|task_id| task_id.to_string()).collect())
}
async fn run_parallel_jobs<T>(jobs: Vec<BatchJob<T>>, max_concurrent: Option<usize>) -> Result<Vec<T>>
where
    T: Send + 'static,
{
    let semaphore = max_concurrent.map(tokio::sync::Semaphore::new).map(Arc::new);
    let handles: Vec<_> = jobs
        .into_iter()
        .map(|job| {
            let semaphore = semaphore.clone();
            tokio::spawn(async move {
                let _permit = match semaphore {
                    Some(semaphore) => Some(
                        semaphore
                            .acquire_owned()
                            .await
                            .context("Batch task semaphore closed")?,
                    ),
                    None => None,
                };
                job.await
            })
        })
        .collect();
    let mut first_err = None;
    let mut results = Vec::new();
    for handle in handles {
        match handle.await.context("Batch task join failure") {
            Ok(Ok(result)) => results.push(result),
            Ok(Err(err)) if first_err.is_none() => first_err = Some(err),
            Err(err) if first_err.is_none() => first_err = Some(err),
            _ => {}
        }
    }
    if let Some(err) = first_err {
        return Err(err);
    }
    Ok(results)
}
async fn wait_for_background_completion(store: &Arc<Store>, task_id: &str) -> Result<()> {
    loop {
        let Some(task) = store.get_task(task_id)? else {
            return Ok(());
        };
        if matches!(task.status, TaskStatus::Done | TaskStatus::Failed) {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
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
async fn dispatch_parallel_with_dependencies(store: Arc<Store>, tasks: &[batch::BatchTask], max_concurrent: Option<usize>) -> Result<Vec<String>> {
    let dependencies = resolve_dependencies(tasks)?;
    let mut started = vec![false; tasks.len()];
    let mut active: Vec<(usize, String)> = Vec::new();
    let mut outcomes = vec![None; tasks.len()];
    let mut task_ids = Vec::new();
    let max_active = max_concurrent.unwrap_or(tasks.len());
    while outcomes.iter().any(Option::is_none) {
        let ready = find_ready_tasks(&store, tasks, &dependencies, &started, &mut outcomes)?;
        let available = max_active.saturating_sub(active.len());
        if available > 0 {
            for dispatch in dispatch_level(store.clone(), tasks, &ready[..ready.len().min(available)]).await? {
                started[dispatch.index] = true;
                match dispatch.task_id {
                    Some(task_id) => {
                        task_ids.push(task_id.clone());
                        active.push((dispatch.index, task_id));
                    }
                    None => outcomes[dispatch.index] = Some(BatchTaskOutcome::Failed),
                }
            }
        }
        if active.is_empty() {
            break;
        }
        wait_for_any_completion(&store, &mut active, &mut outcomes)?;
    }
    Ok(task_ids)
}
async fn dispatch_sequential_with_dependencies(store: Arc<Store>, tasks: &[batch::BatchTask]) -> Result<Vec<String>> {
    let dependencies = resolve_dependencies(tasks)?;
    let mut outcomes = vec![None; tasks.len()];
    let mut task_ids = Vec::new();
    for (task_idx, task) in tasks.iter().enumerate() {
        if let Some(dep_idx) = failed_dependency(task_idx, &dependencies, &outcomes) {
            record_skipped_task(&store, tasks, task_idx, dep_idx)?;
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
async fn dispatch_level(store: Arc<Store>, tasks: &[batch::BatchTask], task_indices: &[usize]) -> Result<Vec<DispatchedTask>> {
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
fn wait_for_any_completion(store: &Arc<Store>, active: &mut Vec<(usize, String)>, outcomes: &mut [Option<BatchTaskOutcome>]) -> Result<()> {
    loop {
        let mut completed = Vec::new();
        for (i, (_, task_id)) in active.iter().enumerate() {
            if let Some(task) = store.get_task(task_id)? {
                if task.status.is_terminal() {
                    completed.push(i);
                }
            }
        }
        if !completed.is_empty() {
            for &i in completed.iter().rev() {
                let (task_idx, task_id) = active.remove(i);
                outcomes[task_idx] = Some(load_task_outcome(store, &task_id)?);
            }
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch;

    #[tokio::test]
    async fn run_parallel_jobs_with_max_concurrent_one_runs_sequentially() {
        let jobs: Vec<BatchJob<(std::time::Instant, std::time::Instant)>> = (0..3).map(|_| {
            Box::pin(async move {
                let start = std::time::Instant::now();
                tokio::time::sleep(tokio::time::Duration::from_millis(40)).await;
                Ok((start, std::time::Instant::now()))
            }) as BatchJob<_>
        }).collect();
        let mut spans = run_parallel_jobs(jobs, Some(1)).await.unwrap();
        spans.sort_by_key(|(start, _)| *start);
        assert_eq!(spans.len(), 3);
        assert!(spans.windows(2).all(|window| window[1].0 >= window[0].1));
    }

    #[test]
    fn task_to_run_args_copies_context() {
        let run_args = task_to_run_args(
            &batch::BatchTask {
                name: None,
                agent: "codex".to_string(),
                prompt: "test".to_string(),
                dir: None,
                output: None,
                model: None,
                worktree: None,
                group: None,
                verify: None,
                max_duration_mins: None,
                context: Some(vec!["src/lib.rs".to_string(), "src/main.rs:run".to_string()]),
                skills: None,
                depends_on: None,
                fallback: None,
                read_only: false,
                budget: false,
            },
            true,
        );

        assert_eq!(
            run_args.context,
            vec!["src/lib.rs".to_string(), "src/main.rs:run".to_string()]
        );
    }
}
