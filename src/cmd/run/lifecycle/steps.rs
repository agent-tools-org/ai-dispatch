// Existing settlement, verification, checklist and postprocess lifecycle steps.
// Exports phase helpers; deps: parent run lifecycle context and task storage.
use super::*;

pub(super) fn run_teardown_phase(task_id: &TaskId, args: &RunArgs, wt_path: Option<&String>) {
    if args.sandbox {
        crate::sandbox::kill_container(task_id.as_str());
    }
    if let Some(wt) = wt_path {
        let _ = crate::worktree::clear_worktree_lock(std::path::Path::new(wt), task_id.as_str());
    }
}

pub(super) fn run_escape_checks_phase(
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

pub(super) async fn run_worktree_settlement_phase(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    effective_dir: Option<&String>,
    pre_task_dirty_paths: Option<&[String]>,
) -> Result<LifecyclePhaseDecision> {
    if args.audit_report_mode && !args.read_only {
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

pub(super) fn run_verify_scope_phase(
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
    crate::verify::enforce_verify_status(store, task_id);
    if !args.read_only && !args.scope.is_empty() {
        run_agent::check_scope_violations(store, task_id, &args.scope, effective_dir);
    }
}

pub(super) fn record_skipped_configured_verify(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    reason: String,
) {
    if args.verify.is_some() {
        run_prompt::record_verify_not_run(store.as_ref(), task_id, reason);
    }
}

pub(super) fn run_checklist_phase(
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

pub(super) fn record_checklist_result(
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

pub(super) fn run_task_postprocess_phase(
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
    capture_final_worktree_state(store.as_ref(), task_id)?;
    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status == TaskStatus::Done {
        handle_done_postprocess(store, task_id, args, &task, agent_kind, prompt_bundle);
    }
    persist_result_file(store, task_id, args, &task, effective_dir);
    let task = store.get_task(task_id.as_str())?.unwrap_or(task);
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

pub(super) fn handle_done_postprocess(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    task: &Task,
    agent_kind: AgentKind,
    prompt_bundle: &run_prompt::PromptBundle,
) {
    // Only a marker that predates this run is stale. One written *during* it was
    // written by this run's own refusal, and success here does not disprove it.
    let model = task.observed_model.as_deref().or(args.model.as_deref());
    rate_limit::clear_rate_limit_for_model_if_stale(
        &agent_kind,
        task.custom_agent_name.as_deref(),
        model,
        task.created_at,
    );
    crate::auth_marker::clear_on_success(agent_kind);
    for memory_id in &prompt_bundle.injected_memory_ids {
        if let Err(err) = store.increment_memory_success(memory_id) {
            aid_error!("[aid] Failed to record memory success for {memory_id}: {err}");
        }
    }
    maybe_flag_empty_worktree_diff(store.as_ref(), task_id, task, args.base_branch.as_deref());
}

pub(super) fn persist_result_file(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    task: &Task,
    effective_dir: Option<&String>,
) {
    // Persist before failed-task worktree cleanup, otherwise the source file may disappear.
    let log_path = task
        .log_path
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::paths::log_path(task_id.as_str()));
    match run_prompt::persist_result_file(
        task_id.as_str(),
        args.result_file.as_deref(),
        effective_dir.map(String::as_str),
        &log_path,
        Some(task.agent_display_name()),
    ) {
        Ok(delivery) => record_missing_report(
            store.as_ref(),
            task_id,
            delivery,
            args.result_file_required == Some(true),
        ),
        Err(err) => aid_warn!("[aid] Failed to persist result file: {err}"),
    }
}

pub(super) fn handle_failed_postprocess(
    _store: &Arc<Store>,
    task_id: &TaskId,
    task: &Task,
    agent_kind: AgentKind,
    agent_display_name: &str,
    effective_dir: Option<&String>,
    _repo_path: Option<&String>,
    runtime_hooks: &[hooks::Hook],
) -> Option<String> {
    let quota_error_message = read_quota_error_message(task_id, &agent_kind);
    crate::auth_marker::record_from_run(task_id.as_str(), agent_kind);
    if let Some(message) = quota_error_message.as_deref() {
        let model = task
            .requested_model
            .as_deref()
            .or(task.observed_model.as_deref());
        rate_limit::mark_rate_limited_for_model(
            &agent_kind,
            task.custom_agent_name.as_deref(),
            model,
            message,
        );
    }
    run_fail_hook(task_id, task, agent_display_name, effective_dir, runtime_hooks);
    quota_error_message
}

pub(super) fn run_fail_hook(
    _task_id: &TaskId,
    task: &Task,
    agent_display_name: &str,
    effective_dir: Option<&String>,
    runtime_hooks: &[hooks::Hook],
) {
    let payload = show::task_hook_json(
        task,
        agent_display_name,
        effective_dir.map(String::as_str),
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
