// Dispatch setup for `aid run` before prompt execution begins.
// Exports: PreparedDispatch and prepare_dispatch().
// Deps: run args/validation helpers, agent registry, project defaults, store.
use anyhow::Result;
use chrono::Local;
use std::{path::{Path, PathBuf}, sync::Arc};
use crate::{agent, paths, project::{self, ProjectConfig}, session};
use crate::{store::{Store, TaskCompletionUpdate}, types::*};
use super::run_dispatch_claim::insert_task_claiming_id;
use super::run_dispatch_resolve::{AgentSetup, apply_project_defaults, resolve_agent_setup};
use super::run_validate::validate_dispatch;
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

struct WorktreeLockGuard { path: Option<String> }

impl WorktreeLockGuard {
    fn new() -> Self { Self { path: None } }
    fn hold(&mut self, path: &str) { self.path = Some(path.to_string()); }
    fn disarm(&mut self) { self.path = None; }
}

impl Drop for WorktreeLockGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            crate::worktree::clear_worktree_lock(Path::new(&path));
        }
    }
}

fn stale_worktree_dir_error(dir: &str, branch: Option<&str>) -> String {
    branch.map(|branch| format!("batch file / task dir missing in worktree: {dir} - workgroup state is stale, run aid worktree remove {branch} and retry"))
        .unwrap_or_else(|| format!("working directory does not exist: {dir}"))
}

pub(super) fn prepare_dispatch(store: &Arc<Store>, args: &mut RunArgs) -> Result<PreparedDispatch> {
    args.prompt = resolve_prompt_input(&args.prompt, args.prompt_file.as_deref())?;
    args.prompt_file = None;
    args.max_duration_mins = resolve_max_duration_mins(args.timeout, args.max_duration_mins);
    let had_explicit_result_file = args.result_file.is_some();
    let detected_project = project::detect_project();
    apply_project_defaults(args, detected_project.as_ref());
    let agent_setup = resolve_agent_setup(store, args)?;
    let explicit_id = args.existing_task_id.is_some();
    let mut task_id = initial_task_id(args)?;
    let mut log_path = paths::log_path(task_id.as_str());
    let workgroup = run_prompt::load_workgroup(store, args.group.as_deref())?;
    let explicit_repo_path = crate::repo_root::resolve_explicit_repo_path(args.repo_root.as_deref(), args.repo.as_deref())?;
    let caller = session::current_caller();
    let mut task = pending_task(args, &agent_setup, &task_id, &log_path, explicit_repo_path.clone(), caller);
    apply_category_and_result_defaults(args, &mut task, had_explicit_result_file);
    for warning in validate_dispatch(args, &agent_setup.agent_kind) {
        aid_warn!("[aid] Warning: {warning}");
    }
    insert_task_claiming_id(store, &mut task, &mut task_id, &mut log_path, explicit_id)?;
    let setup = match setup_worktree(args, detected_project.as_ref(), &agent_setup, &task_id, explicit_repo_path.as_deref()) {
        Ok(setup) => setup,
        Err(err) => {
            fail_claimed_task(store, &task_id, agent_setup.effective_model.as_deref(), &err)?;
            return Err(err);
        }
    };
    if let Err(err) = persist_worktree_setup(store, &task_id, &mut task, &setup) {
        clear_worktree_lock(setup.wt_path.as_deref());
        fail_claimed_task(store, &task_id, agent_setup.effective_model.as_deref(), &err)?;
        return Err(err);
    }
    if setup.emit_gitbutler_setup_hint {
        super::run_dispatch_resolve::insert_gitbutler_setup_hint(store, &task_id);
    }
    prepare_worktree_deps(store, args, &task_id, &agent_setup, &setup)?;
    if should_auto_result_file(args, had_explicit_result_file) {
        let result_file = crate::cmd::report_mode::task_result_file(task_id.as_str());
        args.result_file = Some(result_file.clone());
        aid_info!("[aid] Audit report mode: auto-set --result-file {result_file}");
    }
    Ok(prepared_dispatch(detected_project, agent_setup, task_id, task, log_path, workgroup, setup))
}

fn initial_task_id(args: &RunArgs) -> Result<TaskId> {
    match args.existing_task_id.clone() {
        Some(id) => {
            crate::sanitize::validate_task_id(id.as_str())?;
            Ok(id)
        }
        None => Ok(TaskId::generate()),
    }
}

fn pending_task(
    args: &RunArgs,
    agent_setup: &AgentSetup,
    task_id: &TaskId,
    log_path: &Path,
    repo_path: Option<String>,
    caller: Option<session::CallerSession>,
) -> Task {
    Task {
        id: task_id.clone(), agent: agent_setup.agent_kind, custom_agent_name: agent_setup.custom_agent_name.clone(),
        prompt: args.prompt.clone(), resolved_prompt: None, category: None, status: TaskStatus::Pending,
        parent_task_id: args.parent_task_id.clone(), workgroup_id: args.group.clone(),
        caller_kind: caller.as_ref().map(|item| item.kind.clone()),
        caller_session_id: caller.as_ref().map(|item| item.session_id.clone()),
        agent_session_id: None, repo_path, worktree_path: None, worktree_branch: None, start_sha: None,
        log_path: Some(log_path.to_string_lossy().to_string()), output_path: args.output.clone(),
        tokens: None, prompt_tokens: None, duration_ms: None, model: agent_setup.effective_model.clone(),
        cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
        verify: args.verify.clone(), verify_status: VerifyStatus::Skipped, pending_reason: None,
        read_only: args.read_only, budget: args.budget, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
    }
}

fn apply_category_and_result_defaults(args: &mut RunArgs, task: &mut Task, had_explicit_result_file: bool) {
    let normalized_prompt = task.prompt.trim().to_lowercase();
    let profile = agent::classifier::classify(
        &task.prompt,
        agent::classifier::count_file_mentions(&normalized_prompt),
        task.prompt.chars().count(),
    );
    let report_output = crate::cmd::report_mode::apply_defaults(args, profile.category);
    args.audit_report_mode = crate::cmd::report_mode::skips_dirty_enforcement(&args.prompt, args.read_only, profile.category);
    if report_output && !had_explicit_result_file && args.output.is_none() {
        args.result_file = Some(crate::cmd::report_mode::DEFAULT_AUDIT_RESULT_FILE.to_string());
    }
    task.category = Some(profile.category.label().to_string());
}

fn should_auto_result_file(args: &RunArgs, had_explicit_result_file: bool) -> bool {
    !had_explicit_result_file
        && args.output.is_none()
        && args.result_file.as_deref() == Some(crate::cmd::report_mode::DEFAULT_AUDIT_RESULT_FILE)
}

fn setup_worktree(
    args: &mut RunArgs,
    detected_project: Option<&ProjectConfig>,
    agent_setup: &AgentSetup,
    task_id: &TaskId,
    explicit_repo_path: Option<&str>,
) -> Result<WorktreeSetup> {
    let (wt_path, wt_branch, effective_dir, resolved_repo, fresh_worktree) =
        run_prompt::resolve_worktree_paths(args, explicit_repo_path)?;
    let repo_path = resolved_repo.or_else(|| explicit_repo_path.map(str::to_string));
    let mut lock = WorktreeLockGuard::new();
    if let Some(ref wt) = wt_path {
        if let Err(holder) = crate::worktree::try_acquire_worktree_lock(Path::new(wt), task_id.as_str()) {
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
    store.update_task_worktree(
        task_id.as_str(),
        task.repo_path.as_deref(),
        task.worktree_path.as_deref(),
        task.worktree_branch.as_deref(),
    )
}

fn prepare_worktree_deps(
    store: &Arc<Store>,
    args: &RunArgs,
    task_id: &TaskId,
    agent_setup: &AgentSetup,
    setup: &WorktreeSetup,
) -> Result<()> {
    if args.dry_run { return Ok(()); }
    let (Some(wt), Some(repo)) = (setup.wt_path.as_deref(), setup.repo_path.as_deref()) else {
        return Ok(());
    };
    if let Err(err) = crate::worktree_deps::prepare_worktree_dependencies(
        store, task_id, Path::new(repo), Path::new(wt), args.setup.as_deref(), args.link_deps,
        crate::idle_timeout::idle_timeout_secs_from_env(args.env.as_ref()), setup.fresh_worktree,
    ) {
        clear_worktree_lock(Some(wt));
        fail_claimed_task(store, task_id, agent_setup.effective_model.as_deref(), &err)?;
        return Err(err);
    }
    Ok(())
}

fn clear_worktree_lock(wt_path: Option<&str>) {
    if let Some(wt) = wt_path {
        crate::worktree::clear_worktree_lock(Path::new(wt));
    }
}

fn fail_claimed_task(store: &Store, task_id: &TaskId, model: Option<&str>, err: &anyhow::Error) -> Result<()> {
    store.complete_task_atomic(
        TaskCompletionUpdate {
            id: task_id.as_str(), status: TaskStatus::Failed, tokens: None, duration_ms: 0,
            model, cost_usd: None, exit_code: None,
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
