// Dependency-aware batch scheduling and fallback orchestration.
// Deps: parent batch dispatch helpers and state types.
use super::*;

pub(super) async fn dispatch_with_dependencies(
    store: Arc<Store>,
    tasks: &[batch::BatchTask],
    dependencies: &[Vec<usize>],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&str>,
    repo_root: Option<&str>,
) -> Result<BatchDispatchResult> {
    if tasks.is_empty() {
        return Ok(BatchDispatchResult {
            task_ids: Vec::new(),
            outcomes: Vec::new(),
        });
    }
    warn_rate_limited_agents(tasks);
    let mut run = DependencyRun::new(
        store,
        tasks,
        dependencies,
        max_concurrent,
        auto_fallback,
        shared_dir_path,
        repo_root,
    )?;
    run.schedule().await?;
    run.finish()
}

struct DependencyRun<'a> {
    store: Arc<Store>,
    tasks: &'a [batch::BatchTask],
    dependencies: &'a [Vec<usize>],
    max_concurrent: Option<usize>,
    auto_fallback: bool,
    shared_dir_path: Option<&'a str>,
    repo_root: Option<&'a str>,
    waiting_ids: Vec<String>,
    success_targets: Vec<Option<usize>>,
    failure_targets: Vec<Option<usize>>,
    started: Vec<bool>,
    outcomes: Vec<Option<BatchTaskOutcome>>,
    retried: Vec<bool>,
    triggered: Vec<bool>,
    active: Vec<(usize, String)>,
    task_ids: Vec<String>,
    wait_tracker: ReadyWaitTracker,
}

impl<'a> DependencyRun<'a> {
    fn new(
        store: Arc<Store>,
        tasks: &'a [batch::BatchTask],
        dependencies: &'a [Vec<usize>],
        max_concurrent: Option<usize>,
        auto_fallback: bool,
        shared_dir_path: Option<&'a str>,
        repo_root: Option<&'a str>,
    ) -> Result<Self> {
        let waiting_ids = Self::precreate(&store, tasks);
        let name_map = batch::task_name_map(tasks)?;
        let success_targets =
            resolve_hook_targets(tasks, &name_map, |task| task.on_success.as_deref())?;
        let failure_targets =
            resolve_hook_targets(tasks, &name_map, |task| task.on_fail.as_deref())?;
        let started = vec![false; tasks.len()];
        let outcomes: Vec<Option<BatchTaskOutcome>> = vec![None; tasks.len()];
        let retried = vec![false; tasks.len()];
        let triggered: Vec<bool> = tasks.iter().map(|task| !task.conditional).collect();
        let active: Vec<(usize, String)> = Vec::new();
        let task_ids: Vec<String> = Vec::new();
        let wait_tracker = ReadyWaitTracker::new(tasks);
        Ok(Self {
            store,
            tasks,
            dependencies,
            max_concurrent,
            auto_fallback,
            shared_dir_path,
            repo_root,
            waiting_ids,
            success_targets,
            failure_targets,
            started,
            outcomes,
            retried,
            triggered,
            active,
            task_ids,
            wait_tracker,
        })
    }
    fn precreate(store: &Arc<Store>, tasks: &[batch::BatchTask]) -> Vec<String> {
        // Pre-create all tasks with Waiting status so they're visible in TUI immediately
        tasks
            .iter()
            .enumerate()
            .map(|(i, task)| {
                let id = task
                    .id
                    .as_ref()
                    .filter(|s| crate::support::sanitize::is_valid_task_id(s))
                    .map(|s| crate::types::TaskId(s.clone()))
                    .unwrap_or_else(crate::types::TaskId::generate);
                let agent = task.agent.as_str();
                if let Err(e) = store.insert_waiting_task(
                    id.as_str(),
                    agent,
                    &task.prompt,
                    None,
                    task.group.as_deref(),
                    task.dir.as_deref(),
                    task.worktree.as_deref(),
                    task.model.as_deref(),
                    task.verify.as_deref(),
                    task.read_only,
                    task.budget
                        .is_some_and(crate::types::TaskBudget::uses_budget_mode),
                ) {
                    aid_warn!("[aid] Warning: failed to pre-create task {i}: {e}");
                }
                id.to_string()
            })
            .collect()
    }
}

#[path = "dependencies_phases.rs"]
mod phases;
