// Post-run lifecycle helpers for `aid run`.
// Exports: post_run_lifecycle() and extracted quota/worktree helper functions.
// Deps: run.rs wrappers, hooks, retry/judge flow, store, and task types.
use anyhow::Result;
use std::{path::Path, sync::Arc};
use crate::{agent, hooks, rate_limit, store::Store, types::*};
use crate::cmd::{checklist_scan, judge, retry_logic, show};
use super::{RunArgs, inherit_retry_base_branch, maybe_auto_retry_after_checklist_miss, maybe_auto_retry_after_verify_failure, maybe_cleanup_fast_fail, maybe_judge_retry, maybe_verify, run, run_agent, run_prompt};

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
) -> Result<Option<TaskId>> {
    if args.sandbox {
        crate::sandbox::kill_container(task_id.as_str());
    }
    // Always clear worktree lock when task finishes (success or failure)
    if let Some(wt) = wt_path {
        crate::worktree::clear_worktree_lock(std::path::Path::new(wt));
    }
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
    // Rescue untracked files before verify — ensures verify tests committed state
    if let Some(dir) = effective_dir {
        if !args.read_only {
            match crate::commit::rescue_untracked_files(dir.as_str(), task_id.as_str()) {
                Ok(rescued) if !rescued.is_empty() => {
                    let files_list = rescued.join(", ");
                    aid_warn!("[aid] Rescued {} untracked file(s) into commit: {}", rescued.len(), files_list);
                    let _ = store.insert_event(&TaskEvent {
                        task_id: task_id.clone(),
                        timestamp: chrono::Local::now(),
                        event_kind: EventKind::Milestone,
                        detail: format!("Rescued untracked files: {files_list}"),
                        metadata: None,
                    });
                }
                Err(e) => aid_warn!("[aid] Untracked file rescue failed: {e}"),
                _ => {}
            }
        }
    }
    maybe_verify(
        store,
        task_id,
        args.verify.as_deref(),
        effective_dir.map(String::as_str),
        container_name,
    );
    if !args.read_only && !args.scope.is_empty() {
        run_agent::check_scope_violations(
            store,
            task_id,
            &args.scope,
            effective_dir.map(String::as_str),
        );
    }
    let checklist_result = if !args.checklist.is_empty() {
        match store.get_task(task_id.as_str())? {
            Some(ref task) if task.status == TaskStatus::Done => {
                let output = show::output_text_for_task(store.as_ref(), task_id.as_str(), true)
                    .unwrap_or_default();
                let result = checklist_scan::scan_checklist(&args.checklist, &output);
                if result.all_addressed() {
                    aid_info!("[aid] Checklist: {}", result.summary());
                } else {
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
                Some(result)
            }
            _ => None,
        }
    } else {
        None
    };
    let mut quota_error_message = None;
    if let Some(task) = store.get_task(task_id.as_str())? {
        if task.status == TaskStatus::Done && rate_limit::is_rate_limited(&agent_kind) {
            rate_limit::clear_rate_limit(&agent_kind);
        }
        if task.status == TaskStatus::Done && !prompt_bundle.injected_memory_ids.is_empty() {
            for memory_id in &prompt_bundle.injected_memory_ids {
                if let Err(err) = store.increment_memory_success(memory_id) {
                    aid_error!("[aid] Failed to record memory success for {memory_id}: {err}");
                }
            }
        }
        maybe_flag_empty_worktree_diff(store.as_ref(), task_id, &task);
        maybe_cleanup_fast_fail(store, task_id, &task);
        if task.status == TaskStatus::Failed {
            quota_error_message = read_quota_error_message(task_id);
            if let Some(message) = quota_error_message.as_deref()
                && let Some(clean_message) = rate_limit::extract_rate_limit_message(message)
            {
                rate_limit::mark_rate_limited(&agent_kind, &clean_message);
            }
            let fail_payload = show::task_hook_json(
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
                &fail_payload,
                Some(agent_display_name),
                runtime_hooks,
                false,
            ) {
                aid_error!("[aid] Hook on_fail failed: {err}");
            }
            if !task.read_only
                && let Some(wt) = task.worktree_path.as_deref()
                && Path::new(wt).exists()
            {
                // Don't remove worktree if other active tasks still reference it
                let has_siblings = store.has_active_worktree_siblings(wt, task_id.as_str()).unwrap_or(false);
                if has_siblings {
                    aid_info!("[aid] Preserving worktree {wt} — other active tasks share it");
                } else {
                    let repo = repo_path.map(String::as_str).unwrap_or(".");
                    if let Err(err) = crate::cmd::merge::remove_worktree(repo, wt) {
                        aid_warn!("[aid] Warning: failed to clean up worktree {wt}: {err}");
                    }
                }
            }
        }
    }
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
    if let Err(err) = run_prompt::persist_result_file(task_id.as_str(), args.result_file.as_deref(), effective_dir.map(String::as_str)) {
        aid_warn!("[aid] Failed to persist result file: {err}");
    }
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
pub(crate) fn maybe_flag_empty_worktree_diff(store: &Store, task_id: &TaskId, task: &Task) {
    if task.read_only || task.status != TaskStatus::Done || task.verify_status != VerifyStatus::Skipped {
        return;
    }
    let Some(wt_path) = task.worktree_path.as_deref() else { return; };
    let path = Path::new(wt_path);
    if !path.exists() { return; }
    if let Some(true) = worktree_is_empty_diff(path) {
        aid_warn!("[aid] Warning: agent completed but made no code changes in worktree");
        if let Err(err) = store.update_verify_status(task_id.as_str(), VerifyStatus::EmptyDiff) {
            aid_error!("[aid] Failed to record empty diff status: {err}");
        }
    }
}
pub(crate) fn auto_save_task_output(store: &Store, task: &Task) -> Result<()> {
    let transcript = crate::paths::transcript_path(task.id.as_str());
    let log_path = task.log_path.as_deref().map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::paths::log_path(task.id.as_str()));
    let Some(content) = [transcript, log_path].into_iter()
        .find_map(|path| crate::cmd::show::extract_messages_from_log(&path, true))
        .filter(|content| !content.is_empty()) else { return Ok(()); };
    let output_dir = crate::paths::task_dir(task.id.as_str());
    std::fs::create_dir_all(&output_dir)?;
    let output_path = output_dir.join("output.md");
    std::fs::write(&output_path, &content)?;
    store.update_output_path(task.id.as_str(), &output_path.display().to_string())
}
pub(crate) fn worktree_is_empty_diff(worktree_dir: &Path) -> Option<bool> {
    let head = git_diff_stat_output(worktree_dir, &["diff", "--stat", "HEAD"])?;
    let staged = git_diff_stat_output(worktree_dir, &["diff", "--cached", "--stat"])?;
    Some(head.trim().is_empty() && staged.trim().is_empty())
}
pub(crate) fn git_diff_stat_output(dir: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}
pub(crate) fn rescue_quota_failed_task(store: &Store, task_id: &TaskId, quota_error_message: Option<&str>) {
    if quota_error_message.is_none() { return; }
    let Ok(Some(task)) = store.get_task(task_id.as_str()) else { return; };
    if task.status == TaskStatus::Failed && task.verify_status == VerifyStatus::Passed {
        aid_info!("[aid] Rescuing quota-failed task {} — verify passed", task_id);
        let _ = store.update_task_status(task_id.as_str(), TaskStatus::Done);
    }
}
pub(crate) fn read_quota_error_message(task_id: &TaskId) -> Option<String> {
    let stderr_path = crate::paths::stderr_path(task_id.as_str());
    if let Ok(stderr) = std::fs::read_to_string(&stderr_path) {
        if let Some(line) = find_rate_limit_line(&stderr) { return Some(line); }
    }
    let log_path = crate::paths::log_path(task_id.as_str());
    if let Ok(log) = std::fs::read_to_string(&log_path) {
        if let Some(line) = find_rate_limit_line(&log) { return Some(line); }
    }
    None
}
fn find_rate_limit_line(content: &str) -> Option<String> { content.lines().find_map(rate_limit::extract_rate_limit_message) }
pub(crate) fn take_next_cascade_agent(args: &RunArgs) -> Option<(String, Vec<String>)> {
    let mut cascade = args.cascade.clone();
    if cascade.is_empty() { None } else { Some((cascade.remove(0), cascade)) }
}
