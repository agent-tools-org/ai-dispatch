// Verify and retry helpers for `aid run`.
// Exports fast-fail cleanup, verification execution, and verify-driven retry logic.
use anyhow::Result;
use chrono::Local;
use std::sync::Arc;

use crate::cmd::checklist_scan;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};

use super::RunArgs;

pub(in crate::cmd) fn maybe_cleanup_fast_fail_impl(store: &Store, task_id: &TaskId, task: &Task) {
    let Some(ref wt_path) = task.worktree_path else { return };
    // SANDBOX: refuse to touch anything outside aid-managed worktree paths.
    if !crate::worktree::is_safe_worktree_path(wt_path) {
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
    aid_info!(
        "[aid] No commits on {} — pruned fast-failed worktree dir {} for task {} (failed in <10s)",
        task.worktree_branch.as_deref().unwrap_or("<unknown>"),
        wt_path,
        task_id
    );
}

pub(in crate::cmd) fn maybe_verify_impl(
    store: &Store,
    task_id: &TaskId,
    verify: Option<&str>,
    dir: Option<&str>,
    container_name: Option<&str>,
) {
    let Some(verify_arg) = verify else { return };
    let Some(dir_path) = dir else {
        record_verify_not_run(store, task_id, "no working directory".to_string());
        aid_error!("Verify error: no working directory");
        return;
    };
    let command = if verify_arg == "auto" { None } else { Some(verify_arg) };
    let path = std::path::Path::new(dir_path);
    let task = store.get_task(task_id.as_str()).ok().flatten();
    let worktree_branch = task.as_ref().and_then(|task| task.worktree_branch.clone());
    if !path.is_dir() {
        let detail = stale_worktree_dir_error(dir_path, worktree_branch.as_deref());
        record_verify_not_run(store, task_id, detail.clone());
        aid_error!("Verify error: {detail}");
        return;
    }
    let cargo_target_dir = crate::agent::target_dir_for_worktree(worktree_branch.as_deref());
    match crate::verify::run_verify(path, command, cargo_target_dir.as_deref(), container_name) {
        Ok(result) => {
            let result = match crate::verify::apply_declared_file_check(path, task.as_ref(), result) {
                Ok(result) => result,
                Err(e) => {
                    record_verify_failed(
                        store,
                        task_id,
                        format!("Failed during declared-file verification: {e}"),
                    );
                    aid_error!("Verify error: {e}");
                    return;
                }
            };
            let report = crate::verify::format_verify_report(&result);
            println!("{report}");
            crate::verify::record_verify_status(store, task_id, &result);
            if !result.success {
                let hint = verify_failure_hint(store, task_id, &result.output);
                let detail = match verify_output_excerpt(&result.output) {
                    Some(output) => {
                        format!(
                            "Failed during verification: {}\nOutput: {}{}",
                            result.command,
                            output,
                            hint.as_deref().map(|value| format!("\n{value}")).unwrap_or_default()
                        )
                    }
                    None => format!(
                        "Failed during verification: {}{}",
                        result.command,
                        hint.as_deref().map(|value| format!("\n{value}")).unwrap_or_default()
                    ),
                };
                record_verify_failed(store, task_id, detail);
            }
        }
        Err(e) => {
            record_verify_failed(store, task_id, format!("Failed during verification: {e}"));
            aid_error!("Verify error: {e}");
        }
    }
}

pub(in crate::cmd) fn record_verify_not_run(store: &Store, task_id: &TaskId, reason: String) {
    record_verify_failed(
        store,
        task_id,
        format!("Configured verification did not run: {reason}"),
    );
}

fn record_verify_failed(store: &Store, task_id: &TaskId, detail: String) {
    let _ = store.update_verify_status(task_id.as_str(), VerifyStatus::Failed);
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata: None,
    });
    crate::verify::enforce_verify_status(store, task_id);
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

fn verify_failure_hint(store: &Store, task_id: &TaskId, output: &str) -> Option<String> {
    if !verify_output_suggests_missing_deps(output) {
        return None;
    }
    let worktree = store
        .get_task(task_id.as_str())
        .ok()
        .flatten()
        .and_then(|task| task.worktree_path)?;
    crate::worktree_deps::missing_deps_hint(std::path::Path::new(&worktree)).map(str::to_string)
}

fn verify_output_suggests_missing_deps(output: &str) -> bool {
    let lower = output.to_lowercase();
    [
        "cannot find module",
        "module not found",
        "modulenotfounderror",
        "no module named",
        "command not found",
        "executable file not found",
        "not found in path",
        "npm: not found",
        "pnpm: not found",
        "yarn: not found",
        "env: node: no such file",
        "env: npm: no such file",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
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
    super::apply_retry_target(&task, &mut retry_args)?;
    if task.agent.supports_session_resume() {
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
    super::apply_retry_target(&task, &mut retry_args)?;
    if task.agent.supports_session_resume() {
        retry_args.session_id = task.agent_session_id.clone();
    }
    Box::pin(super::super::run(store.clone(), retry_args)).await.map(Some)
}
#[cfg(test)]
#[path = "run_verify_tests.rs"]
mod tests;
