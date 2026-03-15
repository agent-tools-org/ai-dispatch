// Batch dispatch command for running tasks from a TOML file.
// Exports: BatchArgs, run()
// Deps: crate::batch, crate::cmd::run, crate::cmd::batch_validate, crate::store::Store
use anyhow::{anyhow, Context, Result};
use std::{collections::HashMap, path::Path, sync::Arc};
use crate::batch;
use crate::cmd::run::{self, RunArgs};
use crate::store::Store;
#[path = "batch_validate.rs"]
mod batch_validate;
use batch_validate::{find_ready_tasks, load_task_outcome, resolve_dependencies, task_has_dependencies, task_label, validate_batch_config};
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
    if no_groups_set {
        if let Ok(env_group) = std::env::var("AID_GROUP") {
            for task in &mut config.tasks {
                task.group = Some(env_group.clone());
            }
            eprintln!("[aid] Using workspace {env_group} from AID_GROUP");
        } else if total >= 2 {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("batch");
            let wg = store.create_workgroup(stem, "Auto-created for batch dispatch")?;
            for task in &mut config.tasks {
                task.group = Some(wg.id.to_string());
            }
            eprintln!("[aid] Auto-created workgroup {} for batch {stem}", wg.id);
        }
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
fn task_to_run_args(task: &batch::BatchTask, background: bool, store: &Arc<Store>) -> RunArgs {
    // If team is set and agent is empty/auto, auto-select from team members
    let agent_name = if (task.agent.is_empty() || task.agent == "auto") && task.team.is_some() {
        let team_config = task.team.as_deref().and_then(crate::team::resolve_team);
        let selection_opts = crate::agent::RunOpts {
            dir: task.dir.clone(),
            output: task.output.clone(),
            model: task.model.clone(),
            budget: task.budget,
            read_only: task.read_only,
            context_files: vec![],
            session_id: None,
        };
        let (selected, reason) = crate::agent::select_agent_with_reason(
            &task.prompt, &selection_opts, store, team_config.as_ref(),
        );
        eprintln!("[aid] Batch auto-selected: {selected} (reason: {reason})");
        selected
    } else if task.agent.is_empty() {
        "auto".to_string()
    } else {
        task.agent.clone()
    };
        RunArgs {
            agent_name,
            prompt: task.prompt.clone(),
            dir: task.dir.clone(),
            output: task.output.clone(),
            model: task.model.clone(),
            worktree: task.worktree.clone(),
            group: task.group.clone(),
            verify: task.verify.clone(),
            judge: task.judge.clone(),
            max_duration_mins: task.max_duration_mins.map(|value| value as i64),
        context: task.context.clone().unwrap_or_default(),
        skills: task.skills.clone().unwrap_or_default(),
        hooks: task.hooks.clone().unwrap_or_default(),
        background,
        announce: true,
        cascade: task.fallback.as_deref().map(|f| vec![f.to_string()]).unwrap_or_default(),
        read_only: task.read_only,
        budget: task.budget,
        best_of: task.best_of,
        team: task.team.clone(),
        context_from: task.context_from.clone().unwrap_or_default(),
        ..Default::default()
    }
}
async fn dispatch_parallel(store: Arc<Store>, tasks: &[batch::BatchTask], max_concurrent: Option<usize>) -> Result<Vec<String>> {
    let dependencies = vec![Vec::new(); tasks.len()];
    let max_active = max_concurrent.unwrap_or(tasks.len()).max(1);
    dispatch_with_dependencies(store, tasks, &dependencies, max_active).await
}
async fn dispatch_sequential(store: Arc<Store>, tasks: &[batch::BatchTask]) -> Result<Vec<String>> {
    let dependencies = vec![Vec::new(); tasks.len()];
    dispatch_with_dependencies(store, tasks, &dependencies, 1).await
}
async fn dispatch_parallel_with_dependencies(store: Arc<Store>, tasks: &[batch::BatchTask], max_concurrent: Option<usize>) -> Result<Vec<String>> {
    let dependencies = resolve_dependencies(tasks)?;
    let max_active = max_concurrent.unwrap_or(tasks.len()).max(1);
    dispatch_with_dependencies(store, tasks, &dependencies, max_active).await
}
async fn dispatch_sequential_with_dependencies(store: Arc<Store>, tasks: &[batch::BatchTask]) -> Result<Vec<String>> {
    let dependencies = resolve_dependencies(tasks)?;
    dispatch_with_dependencies(store, tasks, &dependencies, 1).await
}

async fn dispatch_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    dependencies: &[Vec<usize>],
    max_active: usize,
) -> Result<Vec<String>> {
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let name_map = batch::task_name_map(tasks)?;
    let success_targets = resolve_hook_targets(tasks, &name_map, |task| task.on_success.as_deref())?;
    let failure_targets = resolve_hook_targets(tasks, &name_map, |task| task.on_fail.as_deref())?;
    let mut started = vec![false; tasks.len()];
    let mut outcomes = vec![None; tasks.len()];
    let mut triggered: Vec<bool> = tasks.iter().map(|task| !task.conditional).collect();
    let mut active: Vec<(usize, String)> = Vec::new();
    let mut task_ids = Vec::new();
    let max_active = max_active.max(1);
    while outcomes.iter().any(Option::is_none) {
        let ready = find_ready_tasks(
            &store,
            tasks,
            dependencies,
            &started,
            &mut outcomes,
            &triggered,
        )?;
        let available = max_active.saturating_sub(active.len());
        if available > 0 && !ready.is_empty() {
            let dispatch_group: Vec<_> = ready.into_iter().take(available).collect();
            for dispatch in dispatch_level(store.clone(), tasks, &dispatch_group).await? {
                started[dispatch.index] = true;
                match dispatch.task_id {
                    Some(task_id) => {
                        task_ids.push(task_id.clone());
                        active.push((dispatch.index, task_id));
                    }
                    None => {
                        outcomes[dispatch.index] = Some(BatchTaskOutcome::Failed);
                        trigger_conditional(
                            BatchTaskOutcome::Failed,
                            dispatch.index,
                            &mut triggered,
                            &success_targets,
                            &failure_targets,
                        );
                    }
                }
            }
        }
        if active.is_empty() {
            break;
        }
        wait_for_any_completion(
            &store,
            &mut active,
            &mut outcomes,
            &mut triggered,
            &success_targets,
            &failure_targets,
        )?;
    }
    Ok(task_ids)
}

fn resolve_hook_targets<F>(
    tasks: &[batch::BatchTask],
    name_map: &HashMap<&str, usize>,
    selector: F,
) -> Result<Vec<Option<usize>>>
where
    F: Fn(&batch::BatchTask) -> Option<&str>,
{
    tasks
        .iter()
        .map(|task| {
            if let Some(reference) = selector(task) {
                let trimmed = reference.trim();
                let &target_idx = name_map
                    .get(trimmed)
                    .ok_or_else(|| anyhow!("unknown hook target: {trimmed}"))?;
                Ok(Some(target_idx))
            } else {
                Ok(None)
            }
        })
        .collect()
}

fn trigger_conditional(
    outcome: BatchTaskOutcome,
    task_idx: usize,
    triggered: &mut [bool],
    success_targets: &[Option<usize>],
    failure_targets: &[Option<usize>],
) {
    match outcome {
        BatchTaskOutcome::Done => {
            if let Some(target_idx) = success_targets[task_idx] {
                triggered[target_idx] = true;
            }
        }
        BatchTaskOutcome::Failed => {
            if let Some(target_idx) = failure_targets[task_idx] {
                triggered[target_idx] = true;
            }
        }
        BatchTaskOutcome::Skipped => {}
    }
}
async fn dispatch_level(store: Arc<Store>, tasks: &[batch::BatchTask], task_indices: &[usize]) -> Result<Vec<DispatchedTask>> {
    let handles: Vec<_> = task_indices
        .iter()
        .map(|&task_idx| {
            let store = store.clone();
            let run_args = task_to_run_args(&tasks[task_idx], true, &store);
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
fn wait_for_any_completion(
    store: &Arc<Store>,
    active: &mut Vec<(usize, String)>,
    outcomes: &mut [Option<BatchTaskOutcome>],
    triggered: &mut [bool],
    success_targets: &[Option<usize>],
    failure_targets: &[Option<usize>],
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
                trigger_conditional(
                    outcomes[task_idx].unwrap(),
                    task_idx,
                    triggered,
                    success_targets,
                    failure_targets,
                );
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
    use crate::store::Store;
    use std::sync::Arc;

    fn make_task(name: &str, conditional: bool, on_success: Option<&str>) -> batch::BatchTask {
        batch::BatchTask {
            name: Some(name.to_string()),
            agent: "codex".to_string(),
            team: None,
            prompt: "prompt".to_string(),
            dir: None,
            output: None,
            model: None,
            worktree: None,
            group: None,
            best_of: None,
            max_duration_mins: None,
            verify: None,
            judge: None,
            context: None,
            skills: None,
            hooks: None,
            depends_on: None,
            context_from: None,
            fallback: None,
            read_only: false,
            budget: false,
            on_success: on_success.map(str::to_string),
            on_fail: None,
            conditional,
        }
    }


    #[test]
    fn trigger_success_marks_target() {
        let mut triggered = vec![true, false];
        let success_targets = vec![Some(1), None];
        let failure_targets = vec![None, None];
        trigger_conditional(
            BatchTaskOutcome::Done,
            0,
            &mut triggered,
            &success_targets,
            &failure_targets,
        );
        assert!(triggered[1]);
    }

    #[test]
    fn trigger_failure_marks_target() {
        let mut triggered = vec![true, false];
        let success_targets = vec![None, None];
        let failure_targets = vec![Some(1), None];
        trigger_conditional(
            BatchTaskOutcome::Failed,
            0,
            &mut triggered,
            &success_targets,
            &failure_targets,
        );
        assert!(triggered[1]);
    }

    #[test]
    fn conditional_task_stays_dormant_until_triggered() {
        let store = Arc::new(Store::open_memory().unwrap());
        let tasks = vec![make_task("first", false, Some("second")), make_task("second", true, None)];
        let deps = vec![Vec::new(), Vec::new()];
        let started = vec![false; 2];
        let mut outcomes = vec![None; 2];
        let triggered = vec![true, false];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
        assert_eq!(ready, vec![0]);
        let triggered = vec![true, true];
        let ready = find_ready_tasks(&store, &tasks, &deps, &started, &mut outcomes, &triggered).unwrap();
        assert_eq!(ready, vec![0, 1]);
    }

    #[test]
    fn task_to_run_args_copies_context() {
        let store = Arc::new(Store::open_memory().unwrap());
        let run_args = task_to_run_args(
            &batch::BatchTask {
                name: None,
                agent: "codex".to_string(),
                team: None,
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
                hooks: None,
                depends_on: None,
                context_from: None,
                fallback: None,
                read_only: false,
                budget: false,
                judge: None,
                best_of: None,
                on_success: None,
                on_fail: None,
                conditional: false,
            },
            true,
            &store,
        );

        assert_eq!(
            run_args.context,
            vec!["src/lib.rs".to_string(), "src/main.rs:run".to_string()]
        );
    }
}
