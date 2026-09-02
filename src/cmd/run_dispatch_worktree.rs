// Worktree setup and persistence for run dispatch preparation.
// Exports WorktreeSetup plus setup, dependency, and failure-recording helpers.
// Deps: project config, worktree/gitbutler services, Store, and RunArgs.

use anyhow::Result;
use chrono::Local;
use std::path::Path;
use std::sync::Arc;

use crate::project::{self, ProjectConfig};
use crate::store::{Store, TaskCompletionUpdate};
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

use super::run_dispatch_resolve::AgentSetup;
use super::{RunArgs, context_file_from_spec, run_prompt};

pub(super) struct WorktreeSetup {
    pub(super) wt_path: Option<String>,
    pub(super) wt_branch: Option<String>,
    pub(super) effective_dir: Option<String>,
    pub(super) repo_path: Option<String>,
    pub(super) fresh_worktree: bool,
    pub(super) emit_gitbutler_setup_hint: bool,
}

struct WorktreeLockGuard {
    path: Option<String>,
    task_id: String,
}

impl WorktreeLockGuard {
    fn new(task_id: &TaskId) -> Self {
        Self { path: None, task_id: task_id.as_str().to_string() }
    }

    fn hold(&mut self, path: &str) { self.path = Some(path.to_string()); }

    fn disarm(&mut self) { self.path = None; }
}

impl Drop for WorktreeLockGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = crate::worktree::clear_worktree_lock(Path::new(&path), &self.task_id);
        }
    }
}

pub(super) fn setup_worktree(
    store: &Store,
    args: &mut RunArgs,
    detected_project: Option<&ProjectConfig>,
    agent_setup: &AgentSetup,
    task_id: &TaskId,
    explicit_repo_path: Option<&str>,
) -> Result<WorktreeSetup> {
    let (wt_path, wt_branch, effective_dir, resolved_repo, fresh_worktree) =
        run_prompt::resolve_worktree_paths(args, explicit_repo_path)?;
    let repo_path = resolved_repo.or_else(|| explicit_repo_path.map(str::to_string));
    crate::worktree::ensure_requested_worktree_is_isolated(
        args.worktree.as_deref(), repo_path.as_deref(), wt_path.as_deref(),
    )?;
    let mut lock = WorktreeLockGuard::new(task_id);
    if let Some(ref wt) = wt_path {
        if let Err(holder) = crate::worktree::try_acquire_worktree_lock_with_store(
            Path::new(wt), task_id.as_str(), Some(store),
        ) {
            anyhow::bail!(
                "Worktree {wt} is locked by task {holder} — concurrent access prevented. Use separate worktree names for parallel tasks."
            );
        }
        lock.hold(wt);
    }
    let emit_gitbutler_setup_hint = configure_gitbutler(
        args, detected_project, agent_setup, wt_path.as_deref(), repo_path.as_deref(),
    );
    sync_context_files(args, wt_path.as_deref(), repo_path.as_deref());
    ensure_effective_dir(
        effective_dir.as_deref(),
        wt_path.as_deref(),
        wt_branch.as_deref().or(args.worktree.as_deref()),
    )?;
    lock.disarm();
    Ok(WorktreeSetup {
        wt_path, wt_branch, effective_dir, repo_path, fresh_worktree, emit_gitbutler_setup_hint,
    })
}

fn configure_gitbutler(
    args: &mut RunArgs,
    detected_project: Option<&ProjectConfig>,
    agent_setup: &AgentSetup,
    wt_path: Option<&str>,
    repo_path: Option<&str>,
) -> bool {
    if std::env::var("AID_GITBUTLER").map(|value| value == "0").unwrap_or(false) {
        return false;
    }
    let (Some(wt), Some(project), Some(repo)) = (wt_path, detected_project, repo_path) else {
        return false;
    };
    let worktree = Path::new(wt);
    let plan = crate::gitbutler::task_worktree_integration_plan(
        Path::new(repo), worktree, project.gitbutler_mode(), agent_setup.agent_kind.as_str(),
    );
    if plan.install_claude_hooks {
        if let Err(err) = crate::gitbutler::install_claude_hooks(worktree) {
            aid_warn!("[aid] gitbutler: failed to install claude hooks: {err}");
        }
    } else if let Some(command) = plan.on_done_command {
        args.on_done = Some(match args.on_done.take() {
            Some(existing) if !existing.trim().is_empty() => format!("{existing} && {command}"),
            _ => command,
        });
    }
    plan.emit_setup_hint
}

fn sync_context_files(args: &RunArgs, wt_path: Option<&str>, repo_path: Option<&str>) {
    let (Some(wt), Some(repo)) = (wt_path, repo_path) else { return; };
    let context_files = args
        .context
        .iter()
        .map(|spec| context_file_from_spec(spec))
        .collect::<Vec<_>>();
    let synced = crate::worktree::sync_context_files_into_worktree(
        Path::new(repo), Path::new(wt), &context_files,
    );
    if !synced.is_empty() {
        aid_info!(
            "[aid] Synced {} context file(s) into worktree: {}",
            synced.len(), synced.join(", ")
        );
    }
}

fn ensure_effective_dir(dir: Option<&str>, wt_path: Option<&str>, branch: Option<&str>) -> Result<()> {
    if wt_path.is_some()
        && let Some(dir) = dir
        && !Path::new(dir).is_dir()
    {
        anyhow::bail!("{}", stale_worktree_dir_error(dir, branch));
    }
    Ok(())
}

fn stale_worktree_dir_error(dir: &str, branch: Option<&str>) -> String {
    branch
        .map(|branch| format!(
            "batch file / task dir missing in worktree: {dir} - workgroup state is stale; resolve branch {branch} through principal acceptance and custody GC"
        ))
        .unwrap_or_else(|| format!("working directory does not exist: {dir}"))
}

pub(super) fn persist_project_identity(
    store: &Store,
    task_id: &TaskId,
    task: &mut Task,
    detected_project: Option<&ProjectConfig>,
    dir: Option<&str>,
    setup: &WorktreeSetup,
) -> Result<()> {
    task.project_id = resolve_task_project_id(
        detected_project, task.repo_path.as_deref(), dir,
    );
    if let Err(err) = store.update_task_project_id(task_id.as_str(), task.project_id.as_deref()) {
        clear_worktree_lock(setup.wt_path.as_deref(), task_id.as_str());
        fail_claimed_task(store, task_id, &err)?;
        return Err(err);
    }
    Ok(())
}

pub(super) fn resolve_task_project_id(
    detected_project: Option<&ProjectConfig>,
    repo_path: Option<&str>,
    dir: Option<&str>,
) -> Option<String> {
    if let Some(id) = detected_project
        .map(|config| config.id.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return Some(id.to_string());
    }
    if let Some(repo) = repo_path {
        return project::resolve_project_id(Path::new(repo));
    }
    if let Some(dir) = dir {
        return project::resolve_project_id(Path::new(dir));
    }
    project::current_project_id()
}

pub(super) fn persist_worktree_setup(
    store: &Store,
    task_id: &TaskId,
    task: &mut Task,
    setup: &WorktreeSetup,
) -> Result<()> {
    task.repo_path = setup.repo_path.clone();
    task.worktree_path = setup.wt_path.clone();
    task.worktree_branch = setup.wt_branch.clone();
    task.effective_dir = persistable_effective_dir(setup.effective_dir.as_deref());
    store.update_task_worktree(
        task_id.as_str(),
        task.repo_path.as_deref(),
        task.worktree_path.as_deref(),
        task.worktree_branch.as_deref(),
        task.effective_dir.as_deref(),
    )
}

fn persistable_effective_dir(dir: Option<&str>) -> Option<String> {
    let raw = dir.map(str::trim).filter(|value| !value.is_empty()).unwrap_or(".");
    let path = Path::new(raw);
    if path.is_absolute() {
        return Some(raw.to_string());
    }
    match std::env::current_dir() {
        Ok(cwd) => Some(cwd.join(path).to_string_lossy().into_owned()),
        Err(_) if dir.is_some() => Some(raw.to_string()),
        Err(_) => None,
    }
}

pub(super) fn prepare_worktree_deps(
    store: &Arc<Store>,
    args: &RunArgs,
    task_id: &TaskId,
    setup: &WorktreeSetup,
) -> Result<()> {
    if args.dry_run { return Ok(()); }
    let (Some(wt), Some(repo)) = (setup.wt_path.as_deref(), setup.repo_path.as_deref()) else {
        return Ok(());
    };
    if let Err(err) = crate::worktree_deps::prepare_worktree_dependencies(
        store,
        task_id,
        Path::new(repo),
        Path::new(wt),
        args.setup.as_deref(),
        args.link_deps,
        crate::idle_timeout::idle_timeout_secs_from_env(args.env.as_ref()),
        setup.fresh_worktree,
        setup.wt_branch.as_deref(),
    ) {
        clear_worktree_lock(Some(wt), task_id.as_str());
        fail_claimed_task(store, task_id, &err)?;
        return Err(err);
    }
    Ok(())
}

pub(super) fn clear_worktree_lock(wt_path: Option<&str>, task_id: &str) {
    if let Some(wt) = wt_path {
        let _ = crate::worktree::clear_worktree_lock(Path::new(wt), task_id);
    }
}

pub(super) fn fail_claimed_task(
    store: &Store,
    task_id: &TaskId,
    err: &anyhow::Error,
) -> Result<()> {
    crate::task_lifecycle::complete_task_atomic(
        store,
        TaskCompletionUpdate {
            id: task_id.as_str(),
            status: TaskStatus::Failed,
            tokens: None,
            duration_ms: 0,
            observed_model: None,
            attribution_source: None,
            cost_usd: None,
            exit_code: None,
        },
        &TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: format!("Failed during worktree setup: {err}"),
            metadata: None,
        },
    )
}

#[cfg(test)]
#[path = "run_dispatch_effective_dir_tests.rs"]
mod effective_dir_tests;
