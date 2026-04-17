// Post-run lifecycle helpers for `aid run`.
// Exports: post_run_lifecycle() and extracted quota/worktree helper functions.
// Deps: run.rs wrappers, hooks, retry/judge flow, store, and task types.
use anyhow::Result;
use std::{path::Path, sync::Arc};
use crate::{agent, hooks, rate_limit, store::Store, types::*};
use crate::cmd::{checklist_scan, judge, retry_logic, show};
use super::run_dirty::{DirtyWorktreeAction, post_agent_dirty_worktree_cleanup};
use super::run_post::{
    auto_save_task_output, maybe_auto_retry_after_hang, maybe_flag_empty_worktree_diff,
    maybe_run_post_done_audit, read_quota_error_message, rescue_quota_failed_task,
    take_next_cascade_agent, worktree_is_empty_diff,
};
use super::{RunArgs, inherit_retry_base_branch, iterate_config, maybe_auto_retry_after_checklist_miss, maybe_auto_retry_after_verify_failure, maybe_cleanup_fast_fail, maybe_iterate, maybe_judge_retry, maybe_verify, run, run_agent, run_prompt};

#[derive(Debug, Clone, PartialEq, Eq)]
enum LifecyclePhaseDecision {
    Continue,
    Retry(TaskId),
    Stop,
}

impl From<DirtyWorktreeAction> for LifecyclePhaseDecision {
    fn from(action: DirtyWorktreeAction) -> Self {
        match action {
            DirtyWorktreeAction::Continue => Self::Continue,
            DirtyWorktreeAction::Retry(task_id) => Self::Retry(task_id),
            DirtyWorktreeAction::Failed => Self::Stop,
        }
    }
}

pub(crate) async fn post_run_lifecycle(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    agent_kind: AgentKind,
    agent_display_name: &str,
    effective_dir: Option<&String>,
    repo_path: Option<&String>,
    wt_path: Option<&String>,
    container_name: Option<&str>,
    runtime_hooks: &[hooks::Hook],
    prompt_bundle: &run_prompt::PromptBundle,
    pre_verify_status: TaskStatus,
    pre_task_dirty_paths: Option<&[String]>,
) -> Result<Option<TaskId>> {
    run_teardown_phase(task_id, args, wt_path);
    run_escape_checks_phase(args, effective_dir, repo_path, wt_path);
    match run_worktree_settlement_phase(
        store,
        task_id,
        args,
        effective_dir,
        pre_task_dirty_paths,
    )
    .await?
    {
        LifecyclePhaseDecision::Continue => {}
        LifecyclePhaseDecision::Retry(retry_id) => return Ok(Some(retry_id)),
        LifecyclePhaseDecision::Stop => return Ok(None),
    }
    run_verify_scope_phase(
        store,
        task_id,
        args,
        effective_dir.map(String::as_str),
        container_name,
    );
    let checklist_result = run_checklist_phase(store, task_id, args)?;
    let quota_error_message = run_task_postprocess_phase(
        store,
        task_id,
        args,
        agent_kind,
        agent_display_name,
        effective_dir,
        repo_path,
        runtime_hooks,
        prompt_bundle,
    )?;
    rescue_quota_failed_task(store.as_ref(), task_id, quota_error_message.as_deref());
    if let Some(retry_id) = maybe_judge_retry(store, args, task_id).await? {
        return Ok(Some(retry_id));
    }
    if let Some(ref reviewer_agent) = args.peer_review
        && let Some(task) = store.get_task(task_id.as_str())?
        && task.status == TaskStatus::Done
    {
        match judge::peer_review_task(&task, reviewer_agent, &args.prompt).await {
            Ok(review) => {
                aid_info!(
                    "[aid] Peer review by {reviewer_agent}: {}/10 — {}",
                    review.score, review.feedback
                );
                store.save_peer_review(
                    task_id.as_str(),
                    reviewer_agent,
                    review.score,
                    &review.feedback,
                )?;
            }
            Err(e) => aid_error!("[aid] Peer review failed: {e}"),
        }
    }
    run_prompt::notify_task_completion(store, task_id)?;
    if let Some(task) = store.get_task(task_id.as_str())?
        && task.output_path.is_none()
    {
        auto_save_task_output(store.as_ref(), &task)?;
    }
    if let Some(task) = store.get_task(task_id.as_str())? {
        maybe_flag_hollow_output(store.as_ref(), task_id, &task);
    }
    maybe_run_post_done_audit(
        store.as_ref(),
        task_id,
        args,
        effective_dir.map(String::as_str),
        repo_path.map(String::as_str),
    )?;
    super::maybe_auto_gc_after_completion(
        store,
        task_id,
        args,
        repo_path.map(String::as_str),
    )?;
    let Some(task) = store.get_task(task_id.as_str())? else { return Ok(None) };
    let summary_json = serde_json::to_string(&crate::cmd::summary::generate_summary(&task)).unwrap_or_default();
    let _ = store.save_completion_summary(task_id.as_str(), &summary_json);
    if let Some(task) = store.get_task(task_id.as_str())? {
        let done_payload = show::task_hook_json(
            task_id,
            agent_display_name,
            task.status,
            &task.prompt,
            task.worktree_path.as_deref(),
            effective_dir.map(String::as_str),
            task.exit_code,
        );
        if let Err(err) = hooks::run_hooks_with(
            "after_complete",
            &done_payload,
            Some(agent_display_name),
            runtime_hooks,
            false,
        ) {
            aid_error!("[aid] Hook after_complete failed: {err}");
        }
    }
    crate::webhook::fire_task_webhooks(store, task_id.as_str()).await;
    if args.announce {
        let status_hint = if let Some(task) = store.get_task(task_id.as_str())? {
            match task.status {
                TaskStatus::Done => format!("[aid] Next: aid show {task_id} --diff | aid merge {task_id}"),
                TaskStatus::Failed => {
                    let reason = store.latest_error(task_id.as_str())
                        .map(|r| format!("[aid] Reason: {r}\n"))
                        .unwrap_or_default();
                    let next =
                        format!("[aid] Next: aid show {task_id} | aid retry {task_id} -f \"feedback\"");
                    if task.duration_ms.unwrap_or(i64::MAX) < 5000 {
                        let stderr = retry_logic::read_stderr_tail(task_id.as_str(), 3);
                        format!("{reason}{next}\n[aid] Hint: task failed in <5s — check agent binary is installed and --dir points to a valid repo\n[aid] stderr: {stderr}")
                    } else {
                        format!("{reason}{next}")
                    }
                }
                _ => String::new(),
            }
        } else {
            String::new()
        };
        if !status_hint.is_empty() {
            aid_hint!("{status_hint}");
        }
    }
    let iterate_config = iterate_config(args)?;
    if let Some(iterate_config) = iterate_config.as_ref()
        && let Some(retry_id) = maybe_iterate(store, task_id, args, iterate_config).await?
    {
        return Ok(Some(retry_id));
    }
    if iterate_config.is_none() {
        if let Some(retry_id) =
            maybe_auto_retry_after_verify_failure(store, task_id, args, pre_verify_status).await?
        {
            return Ok(Some(retry_id));
        }
        if let Some(retry_id) =
            maybe_auto_retry_after_checklist_miss(store, task_id, args, checklist_result.as_ref()).await?
        {
            return Ok(Some(retry_id));
        }
    }
    if let Some(retry_id) = maybe_auto_retry_after_hang(store, task_id, args).await? {
        return Ok(Some(retry_id));
    }
    crate::verify::enforce_verify_status(store, task_id);
    let completed_normally = if let Some(mut retry_args) =
        retry_logic::prepare_retry(store.clone(), task_id, args).await?
    {
        if let Some(task) = store.get_task(task_id.as_str())? {
            inherit_retry_base_branch(args.dir.as_deref(), &task, &mut retry_args);
        }
        Box::pin(run(store.clone(), retry_args)).await?;
        false
    } else if let Some(task) = store.get_task(task_id.as_str())?
        && task.status == TaskStatus::Failed
        && let Some((next_agent, remaining_cascade)) = take_next_cascade_agent(args)
    {
        aid_info!(
            "[aid] Cascade: trying {} after {} failed",
            next_agent,
            args.agent_name
        );
        let mut cascade_args = args.clone();
        cascade_args.agent_name = next_agent;
        cascade_args.cascade = remaining_cascade;
        cascade_args.parent_task_id = Some(task_id.as_str().to_string());
        Box::pin(run(store.clone(), cascade_args)).await?;
        false
    } else if let Some(task) = store.get_task(task_id.as_str())?
        && task.status == TaskStatus::Failed
        && args.cascade.is_empty()
        && let Some(message) = quota_error_message.as_deref()
        && let Some(clean_message) = rate_limit::extract_rate_limit_message(message)
        && let Some(fallback) = agent::selection::coding_fallback_for(&agent_kind)
    {
        rate_limit::mark_rate_limited(&agent_kind, &clean_message);
        aid_info!(
            "[aid] Quota exhausted for {}, auto-cascading to {}",
            agent_kind.as_str(),
            fallback.as_str()
        );
        let mut cascade_args = args.clone();
        cascade_args.agent_name = fallback.as_str().to_string();
        cascade_args.parent_task_id = Some(task_id.as_str().to_string());
        Box::pin(run(store.clone(), cascade_args)).await?;
        false
    } else {
        true
    };
    if completed_normally { aid_info!("[aid] View in TUI: aid board"); }
    Ok(None)
}

fn run_teardown_phase(task_id: &TaskId, args: &RunArgs, wt_path: Option<&String>) {
    if args.sandbox {
        crate::sandbox::kill_container(task_id.as_str());
    }
    if let Some(wt) = wt_path {
        crate::worktree::clear_worktree_lock(std::path::Path::new(wt));
    }
}

fn run_escape_checks_phase(
    args: &RunArgs,
    effective_dir: Option<&String>,
    repo_path: Option<&String>,
    wt_path: Option<&String>,
) {
    if !args.read_only {
        run_prompt::warn_agent_committed_files_outside_scope(
            &args.scope,
            args.dir.as_ref(),
            effective_dir,
            repo_path,
            wt_path,
        );
    }
    if args.worktree.is_some() {
        run_agent::check_worktree_escape(repo_path.map(String::as_str));
    }
}

async fn run_worktree_settlement_phase(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    effective_dir: Option<&String>,
    pre_task_dirty_paths: Option<&[String]>,
) -> Result<LifecyclePhaseDecision> {
    if args.read_only {
        return Ok(LifecyclePhaseDecision::Continue);
    }
    let Some(dir) = effective_dir else {
        return Ok(LifecyclePhaseDecision::Continue);
    };
    let action = post_agent_dirty_worktree_cleanup(
        store,
        task_id,
        args,
        dir,
        pre_task_dirty_paths,
    )
    .await?;
    Ok(action.into())
}

fn run_verify_scope_phase(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    effective_dir: Option<&str>,
    container_name: Option<&str>,
) {
    maybe_verify(
        store,
        task_id,
        args.verify.as_deref(),
        effective_dir,
        container_name,
    );
    if !args.read_only && !args.scope.is_empty() {
        run_agent::check_scope_violations(store, task_id, &args.scope, effective_dir);
    }
}

fn run_checklist_phase(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
) -> Result<Option<checklist_scan::ChecklistResult>> {
    if args.checklist.is_empty() {
        return Ok(None);
    }
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status != TaskStatus::Done {
        return Ok(None);
    }
    let output = show::output_text_for_task(store.as_ref(), task_id.as_str(), true)
        .unwrap_or_default();
    let result = checklist_scan::scan_checklist(&args.checklist, &output);
    record_checklist_result(store, task_id, &result);
    Ok(Some(result))
}

fn record_checklist_result(
    store: &Arc<Store>,
    task_id: &TaskId,
    result: &checklist_scan::ChecklistResult,
) {
    if result.all_addressed() {
        aid_info!("[aid] Checklist: {}", result.summary());
        return;
    }
    aid_warn!("[aid] Checklist: {} — missing: {}",
        result.summary(), result.missing_items().join(", "));
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: format!("Checklist: {}", result.summary()),
        metadata: None,
    });
}

fn run_task_postprocess_phase(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    agent_kind: AgentKind,
    agent_display_name: &str,
    effective_dir: Option<&String>,
    repo_path: Option<&String>,
    runtime_hooks: &[hooks::Hook],
    prompt_bundle: &run_prompt::PromptBundle,
) -> Result<Option<String>> {
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status == TaskStatus::Done {
        handle_done_postprocess(store, task_id, &task, agent_kind, prompt_bundle);
    }
    maybe_cleanup_fast_fail(store, task_id, &task);
    persist_result_file(task_id, args, effective_dir);
    if task.status == TaskStatus::Failed {
        return Ok(handle_failed_postprocess(
            store,
            task_id,
            &task,
            agent_kind,
            agent_display_name,
            effective_dir,
            repo_path,
            runtime_hooks,
        ));
    }
    Ok(None)
}

fn handle_done_postprocess(
    store: &Arc<Store>,
    task_id: &TaskId,
    task: &Task,
    agent_kind: AgentKind,
    prompt_bundle: &run_prompt::PromptBundle,
) {
    if rate_limit::is_rate_limited(&agent_kind) {
        rate_limit::clear_rate_limit(&agent_kind);
    }
    for memory_id in &prompt_bundle.injected_memory_ids {
        if let Err(err) = store.increment_memory_success(memory_id) {
            aid_error!("[aid] Failed to record memory success for {memory_id}: {err}");
        }
    }
    maybe_flag_empty_worktree_diff(store.as_ref(), task_id, task);
}

fn persist_result_file(
    task_id: &TaskId,
    args: &RunArgs,
    effective_dir: Option<&String>,
) {
    // Persist before failed-task worktree cleanup, otherwise the source file may disappear.
    if let Err(err) = run_prompt::persist_result_file(
        task_id.as_str(),
        args.result_file.as_deref(),
        effective_dir.map(String::as_str),
    ) {
        aid_warn!("[aid] Failed to persist result file: {err}");
    }
}

fn handle_failed_postprocess(
    store: &Arc<Store>,
    task_id: &TaskId,
    task: &Task,
    agent_kind: AgentKind,
    agent_display_name: &str,
    effective_dir: Option<&String>,
    repo_path: Option<&String>,
    runtime_hooks: &[hooks::Hook],
) -> Option<String> {
    let quota_error_message = read_quota_error_message(task_id);
    if let Some(message) = quota_error_message.as_deref()
        && let Some(clean_message) = rate_limit::extract_rate_limit_message(message)
    {
        rate_limit::mark_rate_limited(&agent_kind, &clean_message);
    }
    run_fail_hook(task_id, task, agent_display_name, effective_dir, runtime_hooks);
    cleanup_failed_worktree(store, task_id, task, repo_path);
    quota_error_message
}

fn run_fail_hook(
    task_id: &TaskId,
    task: &Task,
    agent_display_name: &str,
    effective_dir: Option<&String>,
    runtime_hooks: &[hooks::Hook],
) {
    let payload = show::task_hook_json(
        task_id,
        agent_display_name,
        TaskStatus::Failed,
        &task.prompt,
        task.worktree_path.as_deref(),
        effective_dir.map(String::as_str),
        task.exit_code,
    );
    if let Err(err) = hooks::run_hooks_with(
        "on_fail",
        &payload,
        Some(agent_display_name),
        runtime_hooks,
        false,
    ) {
        aid_error!("[aid] Hook on_fail failed: {err}");
    }
}

fn cleanup_failed_worktree(
    store: &Arc<Store>,
    task_id: &TaskId,
    task: &Task,
    repo_path: Option<&String>,
) {
    if task.read_only {
        return;
    }
    let Some(wt) = task.worktree_path.as_deref() else {
        return;
    };
    if !Path::new(wt).exists() {
        return;
    }
    let has_siblings = store
        .has_active_worktree_siblings(wt, task_id.as_str())
        .unwrap_or(false);
    if has_siblings {
        aid_info!("[aid] Preserving worktree {wt} — other active tasks share it");
        return;
    }
    let repo = repo_path.map(String::as_str).unwrap_or(".");
    if let Err(err) = crate::cmd::merge::remove_worktree(repo, wt) {
        aid_warn!("[aid] Warning: failed to clean up worktree {wt}: {err}");
    }
}

pub(crate) fn maybe_flag_hollow_output(store: &Store, task_id: &TaskId, task: &Task) {
    if task.status != TaskStatus::Done || task.verify_status != VerifyStatus::Skipped {
        return;
    }
    if output_content_length(task) >= 200 {
        return;
    }
    let no_worktree_changes = match task.worktree_path.as_deref() {
        Some(path) => worktree_is_empty_diff(Path::new(path)) == Some(true),
        None => true,
    };
    if !no_worktree_changes {
        return;
    }
    aid_warn!("[aid] Warning: agent completed but produced no substantive output");
    if let Err(err) = store.update_delivery_assessment(
        task_id.as_str(),
        Some(DeliveryAssessment::HollowOutput),
    ) {
        aid_error!("[aid] Failed to record hollow output delivery assessment: {err}");
    }
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: "Hollow output: agent produced no substantive deliverable".to_string(),
        metadata: None,
    });
}
fn output_content_length(task: &Task) -> usize {
    if let Some(ref path) = task.output_path {
        if let Ok(content) = std::fs::read_to_string(path) {
            return content.trim().len();
        }
    }
    let auto_path = crate::paths::task_dir(task.id.as_str()).join("output.md");
    std::fs::read_to_string(auto_path).map(|content| content.trim().len()).unwrap_or(0)
}
