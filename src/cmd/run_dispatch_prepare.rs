// Dispatch setup for `aid run` before prompt execution begins.
// Exports: PreparedDispatch and prepare_dispatch().
// Deps: run args/validation helpers, agent registry, project defaults, store.
use anyhow::Result;
use chrono::Local;
use std::{path::{Path, PathBuf}, sync::Arc};
use crate::{agent, paths, project::{self, ProjectConfig}, session};
use crate::{store::Store, types::*};
use super::run_dispatch_claim::insert_task_claiming_id;
use super::run_dispatch_resolve::{AgentSetup, apply_project_defaults, maybe_insert_held_route_event, resolve_agent_setup};
use super::run_dispatch_worktree::{
    WorktreeSetup, clear_worktree_lock, fail_claimed_task, persist_project_identity,
    persist_worktree_setup, prepare_worktree_deps, resolve_task_project_id, setup_worktree,
};
use super::run_task_profile::{
    apply_category_and_result_defaults, persist_declaration, should_auto_result_file,
    validate_egress,
};
use super::run_validate::{validate_command_preflight_with, validate_dispatch};
use super::{RunArgs, resolve_max_duration_mins, resolve_prompt_input, run_prompt};

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

struct DispatchContext {
    detected_project: Option<ProjectConfig>,
    agent_setup: AgentSetup,
    had_explicit_result_file: bool,
}

struct ClaimedDispatch {
    task_id: TaskId,
    task: Task,
    log_path: PathBuf,
    workgroup: Option<Workgroup>,
    explicit_repo_path: Option<String>,
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
    let context = resolve_dispatch_context(store, args)?;
    let claimed = claim_dispatch(store, args, &context, which)?;
    finish_dispatch(store, args, context, claimed)
}

fn resolve_dispatch_context(store: &Arc<Store>, args: &mut RunArgs) -> Result<DispatchContext> {
    let had_explicit_result_file = args
        .result_file_required
        .unwrap_or_else(|| args.result_file.is_some());
    args.result_file_required = Some(had_explicit_result_file);
    let detected_project = match args.dir.as_deref() {
        Some(dir) => project::detect_project_in(Path::new(dir)),
        None => project::detect_project(),
    };
    apply_project_defaults(args, detected_project.as_ref());
    crate::command_diagnostics::validate_run_options(args)?;
    validate_egress(args)?;
    let agent_setup = resolve_agent_setup(store, args)?;
    let agent_name = agent_setup.custom_agent_name.as_deref().unwrap_or_else(|| agent_setup.agent_kind.as_str());
    let mut policy = crate::timeout_policy::TimeoutPolicy::resolve(agent_name, args.idle_timeout_secs, args.max_duration_mins, detected_project.as_ref());
    if let Some(timeout) = args.timeout {
        policy.max_duration = std::time::Duration::from_secs(timeout);
    }
    args.timeout_policy = policy; args.max_duration_mins = Some(policy.max_duration_mins());
    args.env = crate::timeout_policy::env_with_policy(args.env.take(), policy);
    Ok(DispatchContext { detected_project, agent_setup, had_explicit_result_file })
}

fn claim_dispatch<W>(
    store: &Arc<Store>,
    args: &mut RunArgs,
    context: &DispatchContext,
    which: W,
) -> Result<ClaimedDispatch>
where
    W: Fn(&str) -> bool,
{
    let explicit_id = args.existing_task_id.is_some(); let mut task_id = initial_task_id(args)?;
    let mut log_path = paths::log_path(task_id.as_str());
    let workgroup = run_prompt::load_workgroup(store, args.group.as_deref())?;
    let explicit_repo_path = crate::repo_root::resolve_explicit_repo_path(args.repo_root.as_deref(), args.repo.as_deref())?;
    let caller = session::current_caller();
    let mut task = pending_task(
        args,
        &context.agent_setup,
        &task_id,
        &log_path,
        explicit_repo_path.clone(),
        caller,
        context.detected_project.as_ref(),
    );
    apply_category_and_result_defaults(args, &mut task, context.had_explicit_result_file);
    for warning in validate_dispatch(args, &context.agent_setup.agent_kind) {
        aid_warn!("[aid] Warning: {warning}");
    }
    // Refuse unsupported agent/flag combinations before the task row exists so
    // background dispatch cannot return success and die in the worker.
    validate_command_preflight_with(
        context.agent_setup.agent.as_ref(),
        args,
        context.agent_setup.effective_model.as_deref(),
        which,
    )?;
    insert_task_claiming_id(store, &mut task, &mut task_id, &mut log_path, explicit_id)?;
    maybe_insert_held_route_event(store, &task_id, &context.agent_setup, args.dry_run);
    persist_declaration(store, &task_id, args)?;
    Ok(ClaimedDispatch { task_id, task, log_path, workgroup, explicit_repo_path })
}

fn attach_worktree(
    store: &Arc<Store>,
    args: &mut RunArgs,
    context: &DispatchContext,
    claimed: &mut ClaimedDispatch,
) -> Result<WorktreeSetup> {
    let setup = match setup_worktree(
        store,
        args,
        context.detected_project.as_ref(),
        &context.agent_setup,
        &claimed.task_id,
        claimed.explicit_repo_path.as_deref(),
    ) {
        Ok(setup) => setup,
        Err(err) => {
            fail_claimed_task(store, &claimed.task_id, &err)?;
            return Err(err);
        }
    };
    if let Err(err) = persist_worktree_setup(store, &claimed.task_id, &mut claimed.task, &setup) {
        clear_worktree_lock(setup.wt_path.as_deref(), claimed.task_id.as_str());
        fail_claimed_task(store, &claimed.task_id, &err)?;
        return Err(err);
    }
    persist_project_identity(
        store,
        &claimed.task_id,
        &mut claimed.task,
        context.detected_project.as_ref(),
        args.dir.as_deref(),
        &setup,
    )?;
    if setup.emit_gitbutler_setup_hint {
        super::run_dispatch_resolve::insert_gitbutler_setup_hint(store, &claimed.task_id);
    }
    if let Err(err) = super::run_dispatch_guard::ensure_worktree_task_not_repo_root(
        &claimed.task, setup.effective_dir.as_deref(), setup.repo_path.as_deref(),
    ) {
        clear_worktree_lock(setup.wt_path.as_deref(), claimed.task_id.as_str());
        fail_claimed_task(store, &claimed.task_id, &err)?;
        return Err(err);
    }
    prepare_worktree_deps(store, args, &claimed.task_id, &setup)?;
    Ok(setup)
}

fn finish_dispatch(
    store: &Arc<Store>,
    args: &mut RunArgs,
    context: DispatchContext,
    mut claimed: ClaimedDispatch,
) -> Result<PreparedDispatch> {
    let setup = attach_worktree(store, args, &context, &mut claimed)?;
    if should_auto_result_file(args, context.had_explicit_result_file) {
        let result_file = crate::cmd::report_mode::task_result_file(claimed.task_id.as_str());
        args.result_file = Some(result_file.clone());
        args.result_file_required = Some(false);
        aid_info!("[aid] Audit report mode: auto-set --result-file {result_file}");
    }
    let mut dispatch_args = args.clone();
    dispatch_args.model = context.agent_setup.effective_model.clone();
    dispatch_args.model_source = args.model_source;
    store.update_task_dispatch_args(
        claimed.task_id.as_str(), &dispatch_args.dispatch_args_json()?,
    )?;
    Ok(prepared_dispatch(
        context.detected_project,
        context.agent_setup,
        claimed.task_id,
        claimed.task,
        claimed.log_path,
        claimed.workgroup,
        setup,
    ))
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
#[cfg(test)] #[path = "run_dispatch_verify_tests.rs"] mod verify_tests;
#[cfg(test)] #[path = "run_dispatch_preflight_tests.rs"] mod preflight_tests;
