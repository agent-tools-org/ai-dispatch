// Handler for `aid batch <file>` — dispatch multiple tasks from a TOML batch file.
// Supports sequential and parallel (background) dispatch modes.

use anyhow::{Context, Result};
use chrono::Local;
use std::{collections::HashSet, path::Path, sync::Arc};

use crate::batch;
use crate::cmd::run::{self, RunArgs};
use crate::rate_limit;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};

pub struct BatchArgs {
    pub file: String,
    pub parallel: bool,
    pub wait: bool,
    pub max_concurrent: Option<usize>,
}

pub async fn run(store: Arc<Store>, args: BatchArgs) -> Result<()> {
    if args.max_concurrent == Some(0) {
        anyhow::bail!("--max-concurrent must be at least 1");
    }
    let path = Path::new(&args.file);
    let mut config = batch::parse_batch_file(path)
        .with_context(|| format!("Failed to load batch file {}", path.display()))?;
    let total = config.tasks.len();
    let has_dependencies = config.tasks.iter().any(task_has_dependencies);
    rate_limit_precheck(&config.tasks);
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
type BatchJob<T> = std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>;
fn task_to_run_args(task: &batch::BatchTask, background: bool) -> RunArgs {
    RunArgs {
        agent_name: task.agent.clone(),
        prompt: task.prompt.clone(),
        repo: None,
        dir: task.dir.clone(),
        output: task.output.clone(),
        model: task.model.clone(),
        worktree: task.worktree.clone(),
        base_branch: None,
        group: task.group.clone(),
        verify: task.verify.clone(),
        max_duration_mins: task.max_duration_mins.map(|value| value as i64),
        retry: 0,
        context: vec![],
        skills: task.skills.clone().unwrap_or_default(),
        template: None,
        background,
        announce: true,
        parent_task_id: None,
        on_done: None,
        fallback: task.fallback.clone(),
        read_only: false,
        session_id: None,
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
fn rate_limit_precheck(tasks: &[batch::BatchTask]) {
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
async fn dispatch_parallel(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
) -> Result<Vec<String>> {
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
async fn run_parallel_jobs<T>(
    jobs: Vec<BatchJob<T>>,
    max_concurrent: Option<usize>,
) -> Result<Vec<T>>
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
async fn dispatch_parallel_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    max_concurrent: Option<usize>,
) -> Result<Vec<String>> {
    let dependencies = batch::dependency_indices(tasks)?;
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
async fn dispatch_sequential_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
) -> Result<Vec<String>> {
    let dependencies = batch::dependency_indices(tasks)?;
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
fn find_ready_tasks(
    store: &Arc<Store>,
    tasks: &[batch::BatchTask],
    dependencies: &[Vec<usize>],
    started: &[bool],
    outcomes: &mut [Option<BatchTaskOutcome>],
) -> Result<Vec<usize>> {
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
fn wait_for_any_completion(
    store: &Arc<Store>,
    active: &mut Vec<(usize, String)>,
    outcomes: &mut [Option<BatchTaskOutcome>],
) -> Result<()> {
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
fn record_skipped_task(
    store: &Arc<Store>,
    tasks: &[batch::BatchTask],
    task_idx: usize,
    dep_idx: usize,
) -> Result<()> {
    let task_id = insert_skipped_task(store, &tasks[task_idx])?;
    eprintln!(
        "[batch] Skipping task {} ({}) because dependency {} failed",
        task_label(&tasks[task_idx], task_idx),
        task_id,
        task_label(&tasks[dep_idx], dep_idx)
    );
    Ok(())
}
fn insert_skipped_task(store: &Arc<Store>, task: &batch::BatchTask) -> Result<TaskId> {
    let task_id = TaskId::generate();
    let now = Local::now();
    let agent = AgentKind::parse_str(&task.agent)
        .ok_or_else(|| anyhow::anyhow!("Unknown agent '{}'", task.agent))?;
    store.insert_task(&Task {
        id: task_id.clone(),
        agent,
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
        created_at: now,
        resolved_prompt: None,
        completed_at: Some(now),
    })?;
    Ok(task_id)
}
fn load_task_outcome(store: &Arc<Store>, task_id: &str) -> Result<BatchTaskOutcome> {
    let Some(task) = store.get_task(task_id)? else {
        anyhow::bail!("batch task not found after dispatch: {task_id}");
    };
    Ok(match task.status {
        TaskStatus::Done | TaskStatus::Merged => BatchTaskOutcome::Done,
        TaskStatus::Skipped => BatchTaskOutcome::Skipped,
        TaskStatus::Pending | TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Failed => BatchTaskOutcome::Failed,
    })
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
            skills: None,
            depends_on: depends_on.map(|d| d.into_iter().map(String::from).collect()),
            fallback: None,
        }
    }

    #[test]
    fn find_ready_dispatches_when_individual_dep_satisfied() {
        let store = Arc::new(Store::open_memory().unwrap());
        // Diamond DAG: A -> B, A -> C, B -> D, C -> D
        let tasks = vec![
            stub_task("A", None),
            stub_task("B", Some(vec!["A"])),
            stub_task("C", Some(vec!["A"])),
            stub_task("D", Some(vec!["B", "C"])),
        ];
        let deps = vec![
            vec![],        // A: no deps
            vec![0],       // B: depends on A
            vec![0],       // C: depends on A
            vec![1, 2],    // D: depends on B and C
        ];

        // Round 1: nothing started, A is ready
        let mut outcomes: Vec<Option<BatchTaskOutcome>> = vec![None; 4];
        let started = vec![false; 4];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert_eq!(ready, vec![0]); // only A

        // Round 2: A done, B and C become ready simultaneously
        let mut outcomes = vec![Some(BatchTaskOutcome::Done), None, None, None];
        let started = vec![true, false, false, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert_eq!(ready, vec![1, 2]); // B and C ready together

        // Round 3: B done, C still running — D not ready yet
        let mut outcomes = vec![
            Some(BatchTaskOutcome::Done),
            Some(BatchTaskOutcome::Done),
            None,
            None,
        ];
        let started = vec![true, true, true, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert!(ready.is_empty()); // D blocked on C

        // Round 4: both B and C done — D ready
        let mut outcomes = vec![
            Some(BatchTaskOutcome::Done),
            Some(BatchTaskOutcome::Done),
            Some(BatchTaskOutcome::Done),
            None,
        ];
        let started = vec![true, true, true, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert_eq!(ready, vec![3]); // D ready
    }

    #[test]
    fn find_ready_skips_tasks_with_failed_deps() {
        let store = Arc::new(Store::open_memory().unwrap());
        let tasks = vec![
            stub_task("A", None),
            stub_task("B", Some(vec!["A"])),
        ];
        let deps = vec![vec![], vec![0]];
        let mut outcomes = vec![Some(BatchTaskOutcome::Failed), None];
        let started = vec![true, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes).unwrap();
        assert!(ready.is_empty()); // B skipped
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
        let tasks = vec![
            stub_task("first", None),
            stub_task("second", None),
        ];
        // Verify precheck runs without panic; actual warnings go to stderr
        rate_limit_precheck(&tasks);
    }

    #[tokio::test]
    async fn run_parallel_jobs_with_max_concurrent_one_runs_sequentially() {
        let jobs: Vec<BatchJob<(std::time::Instant, std::time::Instant)>> = (0..3)
            .map(|_| {
                Box::pin(async move {
                    let start = std::time::Instant::now();
                    tokio::time::sleep(tokio::time::Duration::from_millis(40)).await;
                    Ok((start, std::time::Instant::now()))
                }) as BatchJob<_>
            })
            .collect();

        let mut spans = run_parallel_jobs(jobs, Some(1)).await.unwrap();
        spans.sort_by_key(|(start, _)| *start);

        assert_eq!(spans.len(), 3);
        assert!(spans.windows(2).all(|window| window[1].0 >= window[0].1));
    }
}
