// Post-run phase ordering and retry/cascade continuation.
// Exports run_lifecycle_phases; deps: parent lifecycle helpers and task state.
use super::*;

pub(super) async fn run_lifecycle_phases(
    mode: LifecycleMode,
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
        LifecyclePhaseDecision::Retry(retry_id) => {
            record_skipped_configured_verify(
                store,
                task_id,
                args,
                format!("dirty worktree rescue dispatched retry {retry_id} before verify"),
            );
            return Ok(Some(retry_id));
        }
        LifecyclePhaseDecision::Stop => {
            record_skipped_configured_verify(
                store,
                task_id,
                args,
                "dirty worktree settlement failed before verify".to_string(),
            );
            if args.read_only
                && let Some(task) = store.get_task(task_id.as_str())?
            {
                persist_result_file(store, task_id, args, &task, effective_dir);
            }
            return Ok(None);
        }
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
        maybe_flag_hollow_output(store.as_ref(), task_id, &task, args.base_branch.as_deref());
    }
    maybe_run_post_done_audit(
        store.as_ref(),
        task_id,
        args,
        effective_dir.map(String::as_str),
        repo_path.map(String::as_str),
    )?;
    let Some(task) = store.get_task(task_id.as_str())? else { return Ok(None) };
    let summary_json = serde_json::to_string(&crate::cmd::summary::generate_summary(&task)).unwrap_or_default();
    let _ = store.save_completion_summary(task_id.as_str(), &summary_json);
    if let Some(task) = store.get_task(task_id.as_str())? {
        let done_payload = show::task_hook_json(
            &task,
            agent_display_name,
            effective_dir.map(String::as_str),
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
    if mode.is_foreground() && args.announce && !args.background {
        if let Some(status_hint) = foreground_status_hint(store.as_ref(), task_id.as_str())? {
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
    if let Some(retry_id) =
        maybe_auto_retry_after_model_unavailable(store, task_id, args).await?
    {
        return Ok(Some(retry_id));
    }
    if let Some(retry_id) =
        maybe_auto_recover_missing_delivery(store, task_id, args).await?
    {
        return Ok(Some(retry_id));
    }
    crate::verify::enforce_verify_status(store, task_id);
    if let Some(mut retry_args) =
        retry_logic::prepare_retry(store.clone(), task_id, args).await?
    {
        if let Some(task) = store.get_task(task_id.as_str())? {
            inherit_retry_base_branch(args.dir.as_deref(), &task, &mut retry_args);
        }
        return Box::pin(run(store.clone(), retry_args)).await.map(Some);
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
        crate::cmd::run::switch_agent(&mut cascade_args, next_agent);
        cascade_args.cascade = remaining_cascade;
        cascade_args.parent_task_id = Some(task_id.as_str().to_string());
        inherit_cascade_target(&mut cascade_args, &task)?;
        return Box::pin(run(store.clone(), cascade_args)).await.map(Some);
    } else if let Some(task) = store.get_task(task_id.as_str())?
        && task.status == TaskStatus::Failed
        && args.cascade.is_empty()
        // Already the provider's own sentence: `read_quota_error_message` reads
        // the stderr and stream channels through `quota_channel` and returns
        // nothing else. Re-matching it here would only be a second opinion on
        // text that has already passed the one gate.
        && let Some(message) = quota_error_message.as_deref()
    {
        if let Some(retry_id) =
            quota_continuation::continue_quota(store, args, &task, message).await?
        {
            return Ok(Some(retry_id));
        }
    }
    if mode.is_foreground() && !args.background {
        aid_info!("[aid] View in TUI: aid board");
    }
    Ok(None)
}

