// Dispatch setup for `aid run` before prompt execution begins.
// Exports: PreparedDispatch and prepare_dispatch().
// Deps: run args/validation helpers, agent registry, project defaults, store.
use anyhow::Result;
use chrono::Local;
use std::{path::{Path, PathBuf}, sync::Arc};
use crate::{agent, paths, project::{self, ProjectConfig}, session};
use crate::{store::{Store, TaskCompletionUpdate}, types::*};
use super::run_dispatch_claim::insert_task_claiming_id;
use super::run_dispatch_resolve::{AgentSetup, apply_project_defaults, maybe_insert_held_route_event, resolve_agent_setup};
use super::run_task_profile::{
    apply_category_and_result_defaults, persist_declaration, should_auto_result_file,
    validate_critical_rigor, validate_egress,
};
use super::run_validate::{validate_command_preflight_with, validate_dispatch};
use super::{RunArgs, context_file_from_spec, resolve_max_duration_mins, resolve_prompt_input, run_prompt};

pub(super) struct PreparedDispatch {
    pub detected_project: Option<ProjectConfig>,
    pub agent_kind: AgentKind,
    pub agent_display_name: String,
    pub requested_skills: Vec<String>,
    pub effective_model: Option<String>,
    pub budget_active: bool,
    pub agent: Box<dyn agent::Agent>,
    pub task_id: TaskId,
    pub task: Task,
    pub log_path: PathBuf,
    pub workgroup: Option<Workgroup>,
    pub repo_path: Option<String>,
    pub wt_path: Option<String>,
    pub effective_dir: Option<String>,
}

struct WorktreeSetup {
    wt_path: Option<String>, wt_branch: Option<String>, effective_dir: Option<String>,
    repo_path: Option<String>, fresh_worktree: bool, emit_gitbutler_setup_hint: bool,
}

struct WorktreeLockGuard { path: Option<String>, task_id: String }

impl WorktreeLockGuard {
    fn new(task_id: &TaskId) -> Self { Self { path: None, task_id: task_id.as_str().to_string() } }
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

fn stale_worktree_dir_error(dir: &str, branch: Option<&str>) -> String {
    branch.map(|branch| format!("batch file / task dir missing in worktree: {dir} - workgroup state is stale; resolve branch {branch} through principal acceptance and custody GC"))
        .unwrap_or_else(|| format!("working directory does not exist: {dir}"))
}

pub(super) fn prepare_dispatch(store: &Arc<Store>, args: &mut RunArgs) -> Result<PreparedDispatch> {
    prepare_dispatch_with(store, args, crate::agent::env::which_exists)
}

pub(super) fn prepare_dispatch_with<W>(
    store: &Arc<Store>,
    args: &mut RunArgs,
    which: W,
) -> Result<PreparedDispatch>
where
    W: Fn(&str) -> bool,
{
    super::run_delegation::apply_nested_delegation(store, args)?;
    args.prompt = resolve_prompt_input(&args.prompt, args.prompt_file.as_deref())?;
    args.prompt_file = None;
    args.max_duration_mins = resolve_max_duration_mins(args.timeout, args.max_duration_mins);
    let had_explicit_result_file = args
        .result_file_required
        .unwrap_or_else(|| args.result_file.is_some());
    args.result_file_required = Some(had_explicit_result_file);
    let detected_project = project::detect_project(); apply_project_defaults(args, detected_project.as_ref());
    validate_critical_rigor(args)?;
    validate_egress(args)?;
    let agent_setup = resolve_agent_setup(store, args)?;
    let agent_name = agent_setup.custom_agent_name.as_deref().unwrap_or_else(|| agent_setup.agent_kind.as_str());
    let policy = crate::timeout_policy::TimeoutPolicy::resolve(agent_name, args.idle_timeout_secs, args.max_duration_mins, detected_project.as_ref());
    args.timeout_policy = policy; args.max_duration_mins = Some(policy.max_duration_mins());
    args.env = crate::timeout_policy::env_with_policy(args.env.take(), policy);
    let explicit_id = args.existing_task_id.is_some(); let mut task_id = initial_task_id(args)?;
    let mut log_path = paths::log_path(task_id.as_str());
    let workgroup = run_prompt::load_workgroup(store, args.group.as_deref())?;
    let explicit_repo_path = crate::repo_root::resolve_explicit_repo_path(args.repo_root.as_deref(), args.repo.as_deref())?;
    let caller = session::current_caller();
    let mut task = pending_task(
        args,
        &agent_setup,
        &task_id,
        &log_path,
        explicit_repo_path.clone(),
        caller,
        detected_project.as_ref(),
    );
    apply_category_and_result_defaults(args, &mut task, had_explicit_result_file);
    for warning in validate_dispatch(args, &agent_setup.agent_kind) {
        aid_warn!("[aid] Warning: {warning}");
    }
    // Refuse unsupported agent/flag combinations before the task row exists so
    // background dispatch cannot return success and die in the worker.
    validate_command_preflight_with(
        agent_setup.agent.as_ref(),
        args,
        agent_setup.effective_model.as_deref(),
        which,
    )?;
    insert_task_claiming_id(store, &mut task, &mut task_id, &mut log_path, explicit_id)?;
    maybe_insert_held_route_event(store, &task_id, &agent_setup);
    persist_declaration(store, &task_id, args)?;
    let setup = match setup_worktree(store, args, detected_project.as_ref(), &agent_setup, &task_id, explicit_repo_path.as_deref()) {
        Ok(setup) => setup,
        Err(err) => {
            fail_claimed_task(store, &task_id, &err)?;
            return Err(err);
        }
    };
    if let Err(err) = persist_worktree_setup(store, &task_id, &mut task, &setup) {
        clear_worktree_lock(setup.wt_path.as_deref(), task_id.as_str());
        fail_claimed_task(store, &task_id, &err)?;
        return Err(err);
    }
    // Re-resolve after worktree/repo are known so main+worktree share identity.
    task.project_id = resolve_task_project_id(
        detected_project.as_ref(),
        task.repo_path.as_deref(),
        args.dir.as_deref(),
    );
    if let Err(err) = store.update_task_project_id(task_id.as_str(), task.project_id.as_deref()) {
        clear_worktree_lock(setup.wt_path.as_deref(), task_id.as_str());
        fail_claimed_task(store, &task_id, &err)?;
        return Err(err);
    }
    if setup.emit_gitbutler_setup_hint { super::run_dispatch_resolve::insert_gitbutler_setup_hint(store, &task_id); }
    if let Err(err) = super::run_dispatch_guard::ensure_worktree_task_not_repo_root(&task, setup.effective_dir.as_deref(), setup.repo_path.as_deref()) {
        clear_worktree_lock(setup.wt_path.as_deref(), task_id.as_str());
        fail_claimed_task(store, &task_id, &err)?;
        return Err(err);
    }
    prepare_worktree_deps(store, args, &task_id, &setup)?;
    if should_auto_result_file(args, had_explicit_result_file) {
        let result_file = crate::cmd::report_mode::task_result_file(task_id.as_str());
        args.result_file = Some(result_file.clone());
        args.result_file_required = Some(false);
        aid_info!("[aid] Audit report mode: auto-set --result-file {result_file}");
    }
    let mut dispatch_args = args.clone();
    dispatch_args.model = agent_setup.effective_model.clone();
    dispatch_args.model_source = args.model_source;
    store.update_task_dispatch_args(task_id.as_str(), &dispatch_args.dispatch_args_json()?)?;
    Ok(prepared_dispatch(detected_project, agent_setup, task_id, task, log_path, workgroup, setup))
}

fn initial_task_id(args: &RunArgs) -> Result<TaskId> {
    let Some(id) = args.existing_task_id.clone() else { return Ok(TaskId::generate()) };
    crate::sanitize::validate_task_id(id.as_str())?;
    Ok(id)
}

fn pending_task(
    args: &RunArgs,
    agent_setup: &AgentSetup,
    task_id: &TaskId,
    log_path: &Path,
    repo_path: Option<String>,
    caller: Option<session::CallerSession>,
    detected_project: Option<&ProjectConfig>,
) -> Task {
    let project_id = resolve_task_project_id(detected_project, repo_path.as_deref(), args.dir.as_deref());
    Task {
        id: task_id.clone(), agent: agent_setup.agent_kind, custom_agent_name: agent_setup.custom_agent_name.clone(),
        prompt: args.prompt.clone(), resolved_prompt: None, category: None, status: TaskStatus::Pending,
        parent_task_id: args.parent_task_id.clone(), workgroup_id: args.group.clone(),
        caller_kind: caller.as_ref().map(|item| item.kind.clone()),
        caller_session_id: caller.as_ref().map(|item| item.session_id.clone()),
        agent_session_id: None, repo_path, project_id, worktree_path: None, effective_dir: None, worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None,
        log_path: Some(log_path.to_string_lossy().to_string()), output_path: args.output.clone(),
        tokens: None, prompt_tokens: None, duration_ms: None, requested_model: agent_setup.effective_model.clone(), observed_model: None, attribution_source: None,
        cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
        verify: args.verify.clone(), verify_status: if verify_required(args.verify.as_deref()) { VerifyStatus::Pending } else { VerifyStatus::Skipped }, pending_reason: None,
        read_only: args.read_only, budget: args.budget, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
    }
}

fn resolve_task_project_id(
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

fn setup_worktree(
    store: &Store, args: &mut RunArgs, detected_project: Option<&ProjectConfig>,
    agent_setup: &AgentSetup, task_id: &TaskId, explicit_repo_path: Option<&str>,
) -> Result<WorktreeSetup> {
    let (wt_path, wt_branch, effective_dir, resolved_repo, fresh_worktree) =
        run_prompt::resolve_worktree_paths(args, explicit_repo_path)?;
    let repo_path = resolved_repo.or_else(|| explicit_repo_path.map(str::to_string));
    crate::worktree::ensure_requested_worktree_is_isolated(args.worktree.as_deref(), repo_path.as_deref(), wt_path.as_deref())?;
    let mut lock = WorktreeLockGuard::new(task_id);
    if let Some(ref wt) = wt_path {
        if let Err(holder) = crate::worktree::try_acquire_worktree_lock_with_store(Path::new(wt), task_id.as_str(), Some(store)) {
            anyhow::bail!("Worktree {wt} is locked by task {holder} — concurrent access prevented. Use separate worktree names for parallel tasks.");
        }
        lock.hold(wt);
    }
    let emit_gitbutler_setup_hint = configure_gitbutler(args, detected_project, agent_setup, wt_path.as_deref(), repo_path.as_deref());
    sync_context_files(args, wt_path.as_deref(), repo_path.as_deref());
    ensure_effective_dir(effective_dir.as_deref(), wt_path.as_deref(), wt_branch.as_deref().or(args.worktree.as_deref()))?;
    lock.disarm();
    Ok(WorktreeSetup { wt_path, wt_branch, effective_dir, repo_path, fresh_worktree, emit_gitbutler_setup_hint })
}

fn configure_gitbutler(
    args: &mut RunArgs, detected_project: Option<&ProjectConfig>,
    agent_setup: &AgentSetup, wt_path: Option<&str>, repo_path: Option<&str>,
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
    let context_files: Vec<String> = args.context.iter().map(|spec| context_file_from_spec(spec)).collect();
    let synced = crate::worktree::sync_context_files_into_worktree(Path::new(repo), Path::new(wt), &context_files);
    if !synced.is_empty() {
        aid_info!("[aid] Synced {} context file(s) into worktree: {}", synced.len(), synced.join(", "));
    }
}

fn ensure_effective_dir(dir: Option<&str>, wt_path: Option<&str>, branch: Option<&str>) -> Result<()> {
    if wt_path.is_some()
        && let Some(dir) = dir
        && !Path::new(dir).is_dir() {
            anyhow::bail!("{}", stale_worktree_dir_error(dir, branch));
        }
    Ok(())
}

fn persist_worktree_setup(store: &Store, task_id: &TaskId, task: &mut Task, setup: &WorktreeSetup) -> Result<()> {
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
    // Effective directories are trimmed before storage; edge spaces are input noise for migrated and new tasks.
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

fn prepare_worktree_deps(
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
        store, task_id, Path::new(repo), Path::new(wt), args.setup.as_deref(), args.link_deps,
        crate::idle_timeout::idle_timeout_secs_from_env(args.env.as_ref()), setup.fresh_worktree, setup.wt_branch.as_deref(),
    ) {
        clear_worktree_lock(Some(wt), task_id.as_str());
        fail_claimed_task(store, task_id, &err)?;
        return Err(err);
    }
    Ok(())
}

fn clear_worktree_lock(wt_path: Option<&str>, task_id: &str) {
    if let Some(wt) = wt_path {
        let _ = crate::worktree::clear_worktree_lock(Path::new(wt), task_id);
    }
}

fn fail_claimed_task(store: &Store, task_id: &TaskId, err: &anyhow::Error) -> Result<()> {
    crate::task_lifecycle::complete_task_atomic(
        store,
        TaskCompletionUpdate {
            id: task_id.as_str(), status: TaskStatus::Failed, tokens: None, duration_ms: 0,
            observed_model: None, attribution_source: None, cost_usd: None, exit_code: None,
        },
        &TaskEvent {
            task_id: task_id.clone(), timestamp: Local::now(), event_kind: EventKind::Error,
            detail: format!("Failed during worktree setup: {err}"), metadata: None,
        },
    )
}

fn prepared_dispatch(
    detected_project: Option<ProjectConfig>,
    agent_setup: AgentSetup,
    task_id: TaskId,
    task: Task,
    log_path: PathBuf,
    workgroup: Option<Workgroup>,
    setup: WorktreeSetup,
) -> PreparedDispatch {
    PreparedDispatch {
        detected_project, agent_kind: agent_setup.agent_kind,
        agent_display_name: agent_setup.agent_display_name,
        requested_skills: agent_setup.requested_skills,
        effective_model: agent_setup.effective_model, budget_active: agent_setup.budget_active,
        agent: agent_setup.agent, task_id, task, log_path, workgroup,
        repo_path: setup.repo_path, wt_path: setup.wt_path, effective_dir: setup.effective_dir,
    }
}

#[cfg(test)] #[path = "run_dispatch_prepare_tests.rs"] mod tests;
#[cfg(test)] #[path = "run_dispatch_effective_dir_tests.rs"] mod effective_dir_tests;
#[cfg(test)] #[path = "run_dispatch_verify_tests.rs"] mod verify_tests;
#[cfg(test)] #[path = "run_dispatch_preflight_tests.rs"] mod preflight_tests;
