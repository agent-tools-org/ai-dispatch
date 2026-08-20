// Handler for `aid merge` — mark done task(s) as merged, optionally by workgroup.
// Exports: run()
// Deps: chrono, crate::store::Store, crate::types

use anyhow::{anyhow, Result};
use chrono::Local;
use std::process::{Command, Stdio};
use std::sync::Arc;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskOutcome, TaskStatus, VerifyStatus};
#[path = "merge/final_branch.rs"]
mod final_branch;
use final_branch::*;
#[path = "merge_git.rs"]
mod merge_git;
use merge_git::*;
#[path = "merge_lanes.rs"]
mod merge_lanes;

pub fn run(store: Arc<Store>, task_id: Option<&str>, group: Option<&str>, approve: bool, check: bool, force: bool, target: Option<&str>, lanes: bool) -> Result<()> {
    run_with_output(store, task_id, group, approve, check, force, target, lanes, true)
}

pub(crate) fn run_quiet(store: Arc<Store>, task_id: Option<&str>, group: Option<&str>, approve: bool, check: bool, force: bool, target: Option<&str>, lanes: bool) -> Result<()> {
    run_with_output(store, task_id, group, approve, check, force, target, lanes, false)
}

fn run_with_output(store: Arc<Store>, task_id: Option<&str>, group: Option<&str>, approve: bool, check: bool, force: bool, target: Option<&str>, lanes: bool, print_summary: bool) -> Result<()> {
    if lanes {
        let Some(group_id) = group else {
            return Err(anyhow!("--lanes requires --group"));
        };
        if check {
            return Err(anyhow!("--lanes does not yet support --check (dry-run); run without --check to apply lanes"));
        }
        if target.is_some() {
            return Err(anyhow!("--lanes cannot be combined with --target; lanes apply to the GitButler workspace of the main repo"));
        }
        return merge_lanes::merge_group_lanes(&store, group_id, force);
    }
    match (task_id, group) {
        (Some(id), _) => merge_single_with_output(&store, id, approve, check, force, target, print_summary),
        (_, Some(group_id)) => merge_group_with_output(&store, group_id, approve, check, force, target, print_summary),
        (None, None) => Err(anyhow!("Provide either a task ID or --group <wg-id>")),
    }
}

fn merge_single(store: &Store, task_id: &str, approve: bool, check: bool, force: bool, target: Option<&str>) -> Result<()> {
    merge_single_with_output(store, task_id, approve, check, force, target, true)
}

fn merge_single_with_output(store: &Store, task_id: &str, approve: bool, check: bool, force: bool, target: Option<&str>, print_summary: bool) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))?;
    let original_status = task.status;
    let outcome = task_outcome(&task);
    validate_merge_outcome(&task, outcome, force)?;
    ensure_task_worktree_is_safe(&task)?;
    let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
    if check { return check_single(task_id, &task, &repo_dir); }

    if !force
        && let Some(wt) = task.worktree_path.as_deref()
        && std::path::Path::new(wt).exists()
    {
        run_verify_in_worktree(wt, task.verify.as_deref());
    }
    if approve {
        match ask_approval(&task)? {
            ApprovalDecision::Merge => {}
            ApprovalDecision::Skip => return Ok(()),
            ApprovalDecision::Retry => {
                aid_info!("[aid] Boss requested retry");
                return Err(anyhow!("Boss requested retry"));
            }
        }
    }
    if let Some(branch) = merge_source_branch(&task) {
        ensure_branch_drift_confirmed(&task, force)?;
        if !force
            && let Some(wt) = task.worktree_path.as_deref()
            && std::path::Path::new(wt).exists()
        {
            auto_commit_uncommitted(wt, branch);
            sync_cargo_lock_before_merge(&repo_dir, wt, branch);
        }
        let ahead = commits_ahead(&repo_dir, branch);
        if ahead == 0 {
            aid_error!("[aid] Error: branch {branch} has 0 commits ahead — nothing to merge");
            aid_hint!("[aid] The agent may not have committed its changes.");
            if let Some(wt) = task.worktree_path.as_deref()
                && std::path::Path::new(wt).exists()
            {
                aid_info!("[aid] Worktree preserved at {wt} for manual recovery");
            }
            return Err(anyhow!("No commits to merge from {branch}"));
        }
        aid_info!("[aid] Branch {branch} has {ahead} commit(s) ahead");
        if let Some(target_branch) = target {
            checkout_branch(&repo_dir, target_branch)?;
        }
        match git_merge_branch(&repo_dir, branch) {
            MergeResult::Merged => {
                aid_info!("[aid] Merged branch {branch} into current branch");
            }
            MergeResult::AlreadyUpToDate => {
                aid_error!("[aid] Error: git merge reported 'Already up to date' despite {ahead} commit(s)");
                aid_warn!("[aid] This may indicate a repo path mismatch. Worktree preserved.");
                return Err(anyhow!("Merge was a no-op — possible repo_path mismatch"));
            }
            MergeResult::Failed(stderr) => {
                aid_warn!("[aid] Warning: git merge {branch} failed:");
                for line in stderr.lines().take(5) {
                    aid_warn!("  {}", line);
                }
                aid_hint!("[aid] Manual merge needed: git merge {branch}");
                let preserved_status = if force { original_status } else { TaskStatus::Done };
                crate::task_lifecycle::restore_after_merge_failure(store, task_id, preserved_status)?;
                return Err(anyhow!("Merge failed — resolve manually, then re-run aid merge {task_id}"));
            }
            MergeResult::StashRestoreFailed(error) => {
                aid_error!("[aid] Error: {error}");
                return Err(anyhow!(error));
            }
        }
    } else {
        if force {
            return Err(anyhow!("Force merge requires a committed worktree branch"));
        }
        let has_changes = Command::new("git")
            .args(["-C", &repo_dir, "status", "--porcelain"])
            .output()
            .ok()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false);
        if has_changes {
            aid_info!("[aid] In-place edit — changes are in your working tree.");
            aid_hint!("[aid] Review: git diff | Revert: git checkout .");
        } else {
            aid_info!("[aid] In-place edit — no uncommitted changes (may already be committed).");
        }
    }
    if force {
        record_force_merge_warning(store, &task)?;
    }
    crate::task_lifecycle::mark_merged(store, task_id)?;
    if print_summary {
        println!("Marked {task_id} as merged");
    }
    Ok(())
}

fn record_force_merge_warning(store: &Store, task: &Task) -> Result<()> {
    let current_task = store
        .get_task(task.id.as_str())?
        .ok_or_else(|| anyhow!("Task '{}' not found", task.id))?;
    let reason = match current_task.verify_status {
        VerifyStatus::Failed => "verification command failed",
        VerifyStatus::InfrastructureFailure => "verification infrastructure failed",
        VerifyStatus::TimedOut => "verification timed out",
        _ => "agent/tests did not complete successfully",
    };
    let detail = format!(
        "Force-merged task {} from status {} — {}",
        current_task.id,
        current_task.status.label(),
        reason,
    );
    aid_warn!("[aid] Warning: {detail}");
    store.insert_event(&TaskEvent {
        task_id: TaskId(task.id.to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: detail.clone(),
        metadata: None,
    })?;
    store.update_principal_merge_override(task.id.as_str(), &detail)?;
    Ok(())
}

fn task_outcome(task: &Task) -> TaskOutcome {
    task.outcome()
}

pub(crate) fn validate_merge_outcome(
    task: &Task,
    outcome: TaskOutcome,
    force: bool,
) -> Result<()> {
    if !matches!(task.status, TaskStatus::Done)
        && !(force && matches!(task.status, TaskStatus::Failed | TaskStatus::Stopped))
    {
        return Err(anyhow!(
            "Task '{}' is {} — only DONE tasks can be marked as merged",
            task.id,
            task.status.label()
        ));
    }
    match outcome {
        TaskOutcome::Verified | TaskOutcome::Delivered => Ok(()),
        TaskOutcome::Unverified(reason) => {
            aid_warn!(
                "[aid] Warning: task '{}' has inconclusive verification ({reason:?})",
                task.id
            );
            if force {
                Ok(())
            } else {
                Err(anyhow!(
                    "Task '{}' has inconclusive verification — use --force to merge",
                    task.id
                ))
            }
        }
        TaskOutcome::Broken => {
            aid_warn!("[aid] Warning: task '{}' has VFAIL status — verify failed before merge", task.id);
            aid_hint!("[aid] Review carefully: aid show {} --diff", task.id);
            if force {
                Ok(())
            } else {
                Err(anyhow!(
                    "Task '{}' verification failed — use --force to merge",
                    task.id
                ))
            }
        }
        TaskOutcome::Failed | TaskOutcome::Stopped if force => Ok(()),
        _ => Err(anyhow!(
            "Task '{}' is {} — only completed tasks can be marked as merged",
            task.id,
            task.status.label()
        )),
    }
}

pub(super) fn is_merge_candidate(task: &Task, outcome: TaskOutcome) -> bool {
    task.status == TaskStatus::Done && outcome.is_merge_candidate()
}

fn merge_group(store: &Store, group_id: &str, approve: bool, check: bool, target: Option<&str>) -> Result<()> {
    merge_group_with_output(store, group_id, approve, check, false, target, true)
}

fn merge_group_with_output(store: &Store, group_id: &str, approve: bool, check: bool, force: bool, target: Option<&str>, print_summary: bool) -> Result<()> {
    let tasks = store.list_tasks_by_group(group_id)?;
    if tasks.is_empty() {
        return Err(anyhow!("No tasks found in group '{group_id}'"));
    }
    // Refuse before prompting or deriving any group repo state: a poisoned first task
    // would otherwise seed that state from a main checkout.
    if let Some(first) = tasks.first() {
        ensure_task_worktree_is_safe(first)?;
    }
    if check {
        return check_group(group_id, &tasks);
    }
    for task in &tasks {
        let outcome = task_outcome(task);
        if is_merge_candidate(task, outcome) {
            validate_merge_outcome(task, outcome, force)?;
        }
    }
    if approve {
        match ask_group_approval(group_id, &tasks)? {
            ApprovalDecision::Merge => {}
            ApprovalDecision::Skip => return Ok(()),
            ApprovalDecision::Retry => {
                aid_info!("[aid] Boss requested retry");
                return Err(anyhow!("Boss requested retry"));
            }
        }
    }
    let mut merged = 0;
    let mut skipped = Vec::new();
    for task in &tasks {
        let outcome = task_outcome(task);
        if !is_merge_candidate(task, outcome) {
            skipped.push(format!("{} ({})", task.id, task.status.label()));
            continue;
        }
        ensure_task_worktree_is_safe(task)?;
        let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
        if let Some(branch) = merge_source_branch(task) {
            ensure_branch_drift_confirmed(task, force)?;
            if let Some(wt) = task.worktree_path.as_deref()
                && std::path::Path::new(wt).exists()
            {
                auto_commit_uncommitted(wt, branch);
                sync_cargo_lock_before_merge(&repo_dir, wt, branch);
            }
            let ahead = commits_ahead(&repo_dir, branch);
            if ahead == 0 {
                aid_warn!("[aid] Warning: {} — branch {branch} has 0 commits, skipping", task.id);
                skipped.push(format!("{} (no commits)", task.id));
                continue;
            }
            if let Some(target_branch) = target {
                checkout_branch(&repo_dir, target_branch)?;
            }
            match git_merge_branch(&repo_dir, branch) {
                MergeResult::Merged => {
                    aid_info!("[aid] Merged branch {branch}");
                    run_post_merge_verify(&repo_dir, task.verify.as_deref());
                }
                MergeResult::AlreadyUpToDate => {
                    aid_warn!("[aid] Warning: {} — merge was no-op despite {ahead} commit(s)", task.id);
                    skipped.push(format!("{} (merge no-op)", task.id));
                    continue;
                }
                MergeResult::Failed(error) => {
                    aid_warn!("[aid] Warning: git merge {branch} failed, skipping {}", task.id);
                    for line in error.lines().take(5) {
                        aid_warn!("  {}", line);
                    }
                    skipped.push(format!("{} (merge conflict)", task.id));
                    continue;
                }
                MergeResult::StashRestoreFailed(error) => {
                    aid_error!("[aid] Error: {error}");
                    return Err(anyhow!(error));
                }
            }
        } else {
            aid_info!("[aid] {} — no worktree, edits applied in-place", task.id);
        }
        crate::task_lifecycle::mark_merged(store, task.id.as_str())?;
        merged += 1;
    }
    if print_summary {
        println!("Merged {merged} task(s) in group {group_id}");
    }
    if !skipped.is_empty() { aid_info!("[aid] Skipped: {}", skipped.join(", ")); }
    Ok(())
}

fn check_single(task_id: &str, task: &Task, repo_dir: &str) -> Result<()> {
    ensure_task_worktree_is_safe(task)?;
    match merge_source_branch(task) {
        Some(branch) => {
            warn_branch_drift(task);
            let result = check_merge(repo_dir, branch);
            print_check_result(task_id, &result);
            if let MergeCheckResult::StashRestoreFailed(error) = result {
                return Err(anyhow!(error));
            }
        }
        None => println!("{task_id}: OK (in-place edit)"),
    }
    Ok(())
}

fn check_group(group_id: &str, tasks: &[Task]) -> Result<()> {
    let mut conflicts = 0;
    for task in tasks {
        ensure_task_worktree_is_safe(task)?;
        let repo_dir = resolve_repo_dir(task.repo_path.as_deref(), task.worktree_path.as_deref());
        match merge_source_branch(task) {
            Some(branch) => {
                warn_branch_drift(task);
                let result = check_merge(&repo_dir, branch);
                if matches!(result, MergeCheckResult::Conflict(_)) {
                    conflicts += 1;
                }
                print_check_result(task.id.as_str(), &result);
                if let MergeCheckResult::StashRestoreFailed(error) = result {
                    return Err(anyhow!(error));
                }
            }
            None => println!("{}: OK (in-place edit)", task.id),
        }
    }
    println!("Checked {} task(s) in group {group_id}; conflicts: {conflicts}", tasks.len());
    Ok(())
}

fn print_check_result(task_id: &str, result: &MergeCheckResult) {
    match result {
        MergeCheckResult::Ok(commits) => println!("{task_id}: OK ({commits} commit(s))"),
        MergeCheckResult::Conflict(files) => println!("{task_id}: CONFLICT ({})", files.join(", ")),
        MergeCheckResult::StashRestoreFailed(error) => println!("{task_id}: ERROR ({error})"),
    }
}

pub(super) fn ensure_task_worktree_is_safe(task: &Task) -> Result<()> {
    let Some(wt) = task.worktree_path.as_deref() else {
        return Ok(());
    };
    if !std::path::Path::new(wt).exists() {
        return Ok(());
    }
    crate::worktree::ensure_consumed_worktree_path_is_isolated(
        task.repo_path.as_deref(),
        wt,
        &format!("recorded worktree path for task {}", task.id),
    )
}

enum ApprovalDecision {
    Merge,
    Retry,
    Skip,
}

fn ask_approval(task: &Task) -> Result<ApprovalDecision> {
    let branch = merge_source_branch(task).unwrap_or("-");
    let prompt = format!(
        "Task {} ready to merge:\n- Route: {}\n- Branch: {}\n\nApprove?",
        task.id,
        task.display_route(),
        branch
    );
    run_approval_prompt(
        &format!("Merge:aid merge {}", task.id),
        &format!("Retry:aid retry {}", task.id),
        &prompt,
    )
}

fn ask_group_approval(group_id: &str, tasks: &[Task]) -> Result<ApprovalDecision> {
    let details = tasks
        .iter()
        .map(|task| format!("- {}: {} ({})", task.id, task.display_route(), merge_source_branch(task).unwrap_or("-")))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!("Group {group_id} ready to merge:\n{details}\n\nApprove?");
    run_approval_prompt(&format!("Merge:aid merge --group {group_id}"), "Retry", &prompt)
}

fn run_approval_prompt(merge_action: &str, retry_action: &str, prompt: &str) -> Result<ApprovalDecision> {
    let actions = format!("{merge_action},{retry_action},Skip");
    let cmd = get_hiboss_command();
    let output = match Command::new(&cmd)
        .args(["ask", "--actions", &actions, "--timeout", "300", prompt])
        .stdout(Stdio::piped())
        .output()
    {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(ApprovalDecision::Merge),
        Err(err) => return Err(err.into()),
    };
    let reply = String::from_utf8_lossy(&output.stdout);
    if reply.contains("Skip") {
        return Ok(ApprovalDecision::Skip);
    }
    if reply.contains("Retry") {
        return Ok(ApprovalDecision::Retry);
    }
    Ok(ApprovalDecision::Merge)
}

#[cfg(test)]
thread_local! {
    static TEST_HIBOSS_COMMAND: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_hiboss_command(cmd: Option<String>) {
    TEST_HIBOSS_COMMAND.with(|cell| *cell.borrow_mut() = cmd);
}

fn get_hiboss_command() -> String {
    #[cfg(test)]
    {
        if let Some(cmd) = TEST_HIBOSS_COMMAND.with(|cell| cell.borrow().clone()) {
            return cmd;
        }
    }
    "hiboss".to_string()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "merge_guard_tests.rs"]
mod merge_guard_tests;
#[cfg(test)]
#[path = "merge/final_branch_tests.rs"]
mod final_branch_tests;
