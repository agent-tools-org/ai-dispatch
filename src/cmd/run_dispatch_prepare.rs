// Dispatch setup for `aid run` before prompt execution begins.
// Exports: PreparedDispatch and prepare_dispatch().
// Deps: run args/validation helpers, agent registry, project defaults, store.
use anyhow::Result;
use chrono::Local;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crate::agent;
use crate::paths;
use crate::project::{self, ProjectConfig};
use crate::session;
use crate::store::{Store, TaskCompletionUpdate};
use crate::types::*;
use super::run_prompt;
use super::run_dispatch_resolve::{apply_project_defaults, resolve_agent_setup};
use super::run_validate::{IdConflict, resolve_id_conflict, validate_dispatch};
use super::{RunArgs, context_file_from_spec, resolve_max_duration_mins, resolve_prompt_input};

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

pub(super) fn prepare_dispatch(store: &Arc<Store>, args: &mut RunArgs) -> Result<PreparedDispatch> {
    args.prompt = resolve_prompt_input(&args.prompt, args.prompt_file.as_deref())?;
    args.prompt_file = None;
    args.max_duration_mins = resolve_max_duration_mins(args.timeout, args.max_duration_mins);
    let had_explicit_result_file = args.result_file.is_some();

    let detected_project = project::detect_project();
    apply_project_defaults(args, detected_project.as_ref());
    let agent_setup = resolve_agent_setup(store, args)?;
    let mut task_id = match args.existing_task_id.clone() {
        Some(id) => {
            crate::sanitize::validate_task_id(id.as_str())?;
            id
        }
        None => TaskId::generate(),
    };
    let log_path = paths::log_path(task_id.as_str());
    let workgroup = run_prompt::load_workgroup(store, args.group.as_deref())?;
    let explicit_repo_path = args.repo.as_deref().map(run_prompt::resolve_repo_path).transpose()?;
    let caller = session::current_caller();
    let worktree_setup = (|| -> Result<_> {
        let (wt_path, wt_branch, effective_dir, resolved_repo, fresh_worktree) =
            run_prompt::resolve_worktree_paths(args, explicit_repo_path.as_deref())?;
        let repo_path = resolved_repo.clone().or(explicit_repo_path.clone());
        if let Some(ref wt) = wt_path {
            if let Some(holder) = crate::worktree::check_worktree_lock(Path::new(wt)) {
                anyhow::bail!(
                    "Worktree {wt} is locked by task {holder} — concurrent access prevented. \
                     Use separate worktree names for parallel tasks."
                );
            }
            crate::worktree::write_worktree_lock(Path::new(wt), task_id.as_str());
        }
        if let Some(ref wt) = wt_path
            && std::env::var("AID_GITBUTLER").map(|value| value != "0").unwrap_or(true)
            && let Some(ref project) = detected_project
            && crate::gitbutler::is_active(project.gitbutler_mode())
        {
            let worktree = Path::new(wt);
            if let Err(err) = crate::gitbutler::ensure_setup(worktree) {
                aid_warn!("[aid] gitbutler: setup failed in {}: {err}", worktree.display());
            } else if crate::gitbutler::agent_uses_claude_hooks(agent_setup.agent_kind.as_str()) {
                if let Err(err) = crate::gitbutler::install_claude_hooks(worktree) {
                    aid_warn!("[aid] gitbutler: failed to install claude hooks: {err}");
                }
            } else {
                let command = crate::gitbutler::on_done_command(worktree);
                args.on_done = Some(match args.on_done.take() {
                    Some(existing) if !existing.trim().is_empty() => format!("{existing} && {command}"),
                    _ => command,
                });
            }
        }
        if let (Some(wt), Some(repo)) = (wt_path.as_deref(), repo_path.as_deref()) {
            let context_files: Vec<String> =
                args.context.iter().map(|spec| context_file_from_spec(spec)).collect();
            let synced = crate::worktree::sync_context_files_into_worktree(
                Path::new(repo),
                Path::new(wt),
                &context_files,
            );
            if !synced.is_empty() {
                aid_info!(
                    "[aid] Synced {} context file(s) into worktree: {}",
                    synced.len(),
                    synced.join(", ")
                );
            }
        }
        Ok((wt_path, wt_branch, effective_dir, repo_path, fresh_worktree))
    })();
    let (wt_path, wt_branch, effective_dir, repo_path, fresh_worktree) = match worktree_setup {
        Ok(paths) => paths,
        Err(err) => {
            let failed_task = Task {
                id: task_id.clone(),
                agent: agent_setup.agent_kind,
                custom_agent_name: agent_setup.custom_agent_name.clone(),
                prompt: args.prompt.clone(),
                resolved_prompt: None,
                category: None,
                status: TaskStatus::Failed,
                parent_task_id: args.parent_task_id.clone(),
                workgroup_id: args.group.clone(),
                caller_kind: caller.as_ref().map(|item| item.kind.clone()),
                caller_session_id: caller.as_ref().map(|item| item.session_id.clone()),
                agent_session_id: None,
                repo_path: explicit_repo_path.clone(),
                worktree_path: None,
                worktree_branch: args.worktree.clone(),
                start_sha: None,
                log_path: Some(log_path.to_string_lossy().to_string()),
                output_path: args.output.clone(),
                tokens: None,
                prompt_tokens: None,
                duration_ms: Some(0),
                model: agent_setup.effective_model.clone(),
                cost_usd: None,
                exit_code: None,
                created_at: Local::now(),
                completed_at: Some(Local::now()),
                verify: args.verify.clone(),
                verify_status: VerifyStatus::Skipped,
                pending_reason: None,
                read_only: args.read_only,
                budget: args.budget,
            };
            let _ = store.insert_task(&failed_task);
            run_prompt::insert_phase_error_event(
                store,
                &task_id,
                "worktree setup",
                &err.to_string(),
                None,
            );
            return Err(err);
        }
    };
    let mut task = Task {
        id: task_id.clone(),
        agent: agent_setup.agent_kind,
        custom_agent_name: agent_setup.custom_agent_name.clone(),
        prompt: args.prompt.clone(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Pending,
        parent_task_id: args.parent_task_id.clone(),
        workgroup_id: args.group.clone(),
        caller_kind: caller.as_ref().map(|item| item.kind.clone()),
        caller_session_id: caller.as_ref().map(|item| item.session_id.clone()),
        agent_session_id: None,
        repo_path: repo_path.clone(),
        worktree_path: wt_path.clone(),
        worktree_branch: wt_branch,
        start_sha: None,
        log_path: Some(log_path.to_string_lossy().to_string()),
        output_path: args.output.clone(),
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: agent_setup.effective_model.clone(),
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: args.verify.clone(),
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: args.read_only,
        budget: args.budget,
    };
    let normalized_prompt = task.prompt.trim().to_lowercase();
    let profile = agent::classifier::classify(
        &task.prompt,
        agent::classifier::count_file_mentions(&normalized_prompt),
        task.prompt.chars().count(),
    );
    let auto_result_file = crate::cmd::report_mode::apply_defaults(args, profile.category)
        && !had_explicit_result_file
        && args.output.is_none()
        && args.result_file.as_deref() == Some(crate::cmd::report_mode::DEFAULT_AUDIT_RESULT_FILE);
    task.category = Some(profile.category.label().to_string());
    for warning in validate_dispatch(args, &agent_setup.agent_kind) {
        aid_warn!("[aid] Warning: {warning}");
    }
    if args.existing_task_id.is_some() {
        match resolve_id_conflict(store, task_id.as_str())? {
            IdConflict::None => store.insert_task(&task)?,
            IdConflict::ReplaceWaiting => store.replace_waiting_task(&task)?,
            IdConflict::Running => {
                anyhow::bail!(
                    "Task '{}' is still running. Stop it first: aid stop {}",
                    task_id, task_id
                );
            }
            IdConflict::AutoSuffix(new_id) => {
                aid_info!("[aid] ID '{}' already exists, using '{}'", task_id, new_id);
                task.id = TaskId(new_id.clone());
                task_id = TaskId(new_id);
                store.insert_task(&task)?;
            }
        }
    } else {
        store.insert_task(&task)?;
    }
    if !args.dry_run
        && let (Some(wt), Some(repo)) = (wt_path.as_deref(), repo_path.as_deref())
        && let Err(err) = crate::worktree_deps::prepare_worktree_dependencies(
            store,
            &task_id,
            Path::new(repo),
            Path::new(wt),
            args.setup.as_deref(),
            args.link_deps,
            crate::idle_timeout::idle_timeout_secs_from_env(args.env.as_ref()),
            fresh_worktree,
        )
    {
        crate::worktree::clear_worktree_lock(Path::new(wt));
        store.complete_task_atomic(
            TaskCompletionUpdate {
                id: task_id.as_str(),
                status: TaskStatus::Failed,
                tokens: None,
                duration_ms: 0,
                model: agent_setup.effective_model.as_deref(),
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
        )?;
        return Err(err);
    }
    if auto_result_file {
        let result_file = crate::cmd::report_mode::task_result_file(task_id.as_str());
        args.result_file = Some(result_file.clone());
        aid_info!("[aid] Audit report mode: auto-set --result-file {result_file}");
    }
    Ok(PreparedDispatch {
        detected_project,
        agent_kind: agent_setup.agent_kind,
        agent_display_name: agent_setup.agent_display_name,
        requested_skills: agent_setup.requested_skills,
        effective_model: agent_setup.effective_model,
        budget_active: agent_setup.budget_active,
        agent: agent_setup.agent,
        task_id,
        task,
        log_path,
        workgroup,
        repo_path,
        wt_path,
        effective_dir,
    })
}
#[cfg(test)]
#[path = "run_dispatch_prepare_tests.rs"]
mod tests;
