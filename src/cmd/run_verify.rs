// Verify and retry helpers for `aid run`.
// Exports fast-fail cleanup, verification execution, and verify-driven retry logic.
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;

use crate::cmd::checklist_scan;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus};

use super::RunArgs;

pub(in crate::cmd) fn maybe_cleanup_fast_fail_impl(store: &Store, task_id: &TaskId, task: &Task) {
    let Some(ref wt_path) = task.worktree_path else { return };
    // SANDBOX: refuse to touch anything outside /tmp/aid-wt-*
    if !crate::cmd::merge::merge_git::is_safe_worktree_path(wt_path) {
        aid_warn!("[aid] SAFETY: refusing to remove '{}' — not an aid worktree path", wt_path);
        return;
    }
    let path = std::path::Path::new(wt_path);
    if !path.exists() { return }
    let Some(task) = store.get_task(task_id.as_str()).ok().flatten() else { return };
    if task.status != TaskStatus::Failed { return }
    let Some(duration_ms) = task.duration_ms else { return };
    if duration_ms > 10_000 { return }
    if crate::worktree::branch_has_commits_ahead_of_main(path, task.worktree_branch.as_deref().unwrap_or("unknown")).unwrap_or(true) { return; }
    let Some(repo_dir) = task.repo_path.as_deref() else {
        aid_warn!("[aid] Warning: skipping fast-fail cleanup for {} — missing repo_path", task_id);
        return;
    };
    let _ = std::process::Command::new("git")
        .args(["-C", repo_dir, "worktree", "remove", "--force", wt_path])
        .output();
    aid_info!("[aid] Cleaned up worktree for fast-failed task {}", task_id);
}

pub(in crate::cmd) fn maybe_verify_impl(
    store: &Store,
    task_id: &TaskId,
    verify: Option<&str>,
    dir: Option<&str>,
    container_name: Option<&str>,
) {
    let Some(verify_arg) = verify else { return };
    let Some(dir_path) = dir else { println!("Verify skipped: no working directory"); return; };
    let command = if verify_arg == "auto" { None } else { Some(verify_arg) };
    let path = std::path::Path::new(dir_path);
    let task = store.get_task(task_id.as_str()).ok().flatten();
    let worktree_branch = task.as_ref().and_then(|task| task.worktree_branch.clone());
    if !path.is_dir() {
        let detail = stale_worktree_dir_error(dir_path, worktree_branch.as_deref());
        let event = TaskEvent {
            task_id: task_id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: detail.clone(),
            metadata: None,
        };
        let _ = store.insert_event(&event);
        aid_error!("Verify error: {detail}");
        return;
    }
    let cargo_target_dir = crate::agent::target_dir_for_worktree(worktree_branch.as_deref());
    match crate::verify::run_verify(path, command, cargo_target_dir.as_deref(), container_name) {
        Ok(result) => {
            let report = crate::verify::format_verify_report(&result);
            println!("{report}");
            crate::verify::record_verify_status(store, task_id, &result);
            if !result.success {
                let detail = match verify_output_excerpt(&result.output) {
                    Some(output) => {
                        format!(
                            "Failed during verification: {}\nOutput: {}",
                            result.command, output
                        )
                    }
                    None => format!("Failed during verification: {}", result.command),
                };
                let event = TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: Local::now(),
                    event_kind: EventKind::Error,
                    detail,
                    metadata: None,
                };
                let _ = store.insert_event(&event);
            }
        }
        Err(e) => {
            let event = TaskEvent {
                task_id: task_id.clone(),
                timestamp: Local::now(),
                event_kind: EventKind::Error,
                detail: format!("Failed during verification: {e}"),
                metadata: None,
            };
            let _ = store.insert_event(&event);
            aid_error!("Verify error: {e}");
        }
    }
}

fn stale_worktree_dir_error(dir: &str, branch: Option<&str>) -> String {
    match branch {
        Some(branch) => format!(
            "batch file / task dir missing in worktree: {dir} - workgroup state is stale, run aid worktree remove {branch} and retry"
        ),
        None => format!("working directory does not exist: {dir}"),
    }
}

fn verify_output_excerpt(output: &str) -> Option<String> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let start = lines.len().saturating_sub(8);
    let excerpt = lines[start..].join(" | ");
    Some(if excerpt.chars().count() > 400 {
        let mut truncated: String = excerpt.chars().take(400).collect();
        truncated.push_str("...");
        truncated
    } else {
        excerpt
    })
}

pub(in crate::cmd) async fn maybe_auto_retry_after_verify_failure_impl(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    pre_verify_status: TaskStatus,
) -> Result<Option<TaskId>> {
    if args.verify.is_none() || args.retry == 0 || pre_verify_status != TaskStatus::Done {
        return Ok(None);
    }
    let Some(task) = store.get_task(task_id.as_str())? else { return Ok(None) };
    if task.verify_status != crate::types::VerifyStatus::Failed {
        return Ok(None);
    }

    aid_warn!(
        "[aid] Verify failed, auto-retrying ({} retries left)",
        args.retry - 1
    );

    let mut retry_args = args.clone();
    retry_args.prompt = format!(
        "[Previous attempt feedback]\n{}\n\n[Original task]\n{}",
        super::VERIFY_RETRY_FEEDBACK,
        task.prompt
    );
    retry_args.retry = args.retry.saturating_sub(1);
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    retry_args.output = task
        .output_path
        .clone()
        .or_else(|| retry_args.output.clone());
    retry_args.model = task.model.clone().or_else(|| retry_args.model.clone());
    retry_args.verify = task.verify.clone();
    retry_args.read_only = task.read_only;
    retry_args.budget = task.budget;
    retry_args.background = false;
    let (dir, worktree) = super::retry_target(&task);
    retry_args.dir = dir.or_else(|| retry_args.dir.clone());
    retry_args.worktree = worktree.or_else(|| retry_args.worktree.clone());
    if task.agent == AgentKind::OpenCode {
        retry_args.session_id = task.agent_session_id.clone();
    }

    Box::pin(super::super::run(store.clone(), retry_args)).await.map(Some)
}

pub(in crate::cmd) async fn maybe_auto_retry_after_checklist_miss_impl(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &super::RunArgs,
    checklist_result: Option<&checklist_scan::ChecklistResult>,
) -> Result<Option<TaskId>> {
    if args.checklist.is_empty() || args.retry == 0 {
        return Ok(None);
    }
    let Some(result) = checklist_result else { return Ok(None) };
    if result.all_addressed() {
        return Ok(None);
    }
    let Some(task) = store.get_task(task_id.as_str())? else { return Ok(None) };
    if task.status != TaskStatus::Done {
        return Ok(None);
    }
    aid_warn!(
        "[aid] Checklist incomplete, auto-retrying ({} retries left)",
        args.retry.saturating_sub(1)
    );
    let missing = result.missing_items().join("\n");
    let mut retry_args = args.clone();
    retry_args.prompt = format!(
        "[Checklist items not addressed]\nYou MUST address these items:\n{missing}\n\n[Original task]\n{}",
        task.prompt
    );
    retry_args.retry = args.retry.saturating_sub(1);
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    retry_args.output = task.output_path.clone().or_else(|| retry_args.output.clone());
    retry_args.model = task.model.clone().or_else(|| retry_args.model.clone());
    retry_args.verify = task.verify.clone();
    retry_args.read_only = task.read_only;
    retry_args.budget = task.budget;
    retry_args.background = false;
    let (dir, worktree) = super::retry_target(&task);
    retry_args.dir = dir.or_else(|| retry_args.dir.clone());
    retry_args.worktree = worktree.or_else(|| retry_args.worktree.clone());
    if task.agent == AgentKind::OpenCode {
        retry_args.session_id = task.agent_session_id.clone();
    }
    Box::pin(super::super::run(store.clone(), retry_args)).await.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;
    use crate::types::{Task, VerifyStatus};

    #[test]
    fn verify_output_excerpt_keeps_last_lines() {
        let output = (1..=10)
            .map(|idx| format!("line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let excerpt = verify_output_excerpt(&output).unwrap();

        assert_eq!(
            excerpt,
            "line 3 | line 4 | line 5 | line 6 | line 7 | line 8 | line 9 | line 10"
        );
    }

    #[test]
    fn maybe_verify_reports_stale_worktree_when_dir_is_missing() {
        let store = Store::open_memory().unwrap();
        let task_id = TaskId("t-stale-verify".to_string());
        store
            .insert_task(&Task {
                id: task_id.clone(),
                agent: AgentKind::Codex,
                custom_agent_name: None,
                prompt: "prompt".to_string(),
                resolved_prompt: None,
                category: None,
                status: TaskStatus::Done,
                parent_task_id: None,
                workgroup_id: Some("wg-stale".to_string()),
                caller_kind: None,
                caller_session_id: None,
                agent_session_id: None,
                repo_path: None,
                worktree_path: Some("/tmp/aid-wt-feat-stale".to_string()),
                worktree_branch: Some("feat/stale".to_string()),
                start_sha: None,
                log_path: None,
                output_path: None,
                tokens: None,
                prompt_tokens: None,
                duration_ms: None,
                model: None,
                cost_usd: None,
                exit_code: None,
                created_at: Local::now(),
                completed_at: None,
                verify: Some("auto".to_string()),
                verify_status: VerifyStatus::Skipped,
                pending_reason: None,
                read_only: false,
                budget: false,
            })
            .unwrap();

        maybe_verify_impl(
            &store,
            &task_id,
            Some("auto"),
            Some("/tmp/aid-wt-feat-stale/.aid/batches"),
            None,
        );

        let error = store.latest_error(task_id.as_str()).unwrap();
        assert!(error.contains("batch file / task dir missing in worktree"));
        assert!(error.contains("aid worktree remove feat/stale"));
    }
}
