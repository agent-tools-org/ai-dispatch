// Dirty-worktree enforcement shared by foreground and background runs.
// Exports rescue/retry/final-assertion helpers plus the post-run action enum.
// Deps: commit helpers, run retry flow, store, and task types.

use anyhow::Result;
use std::{path::Path, sync::Arc};

use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

use super::{RunArgs, retry_target, run};
#[path = "read_only.rs"]
mod read_only;
pub(super) use read_only::capture_baseline;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirtyWorktreeAction {
    Continue,
    Retry(TaskId),
    Failed,
}

pub(crate) async fn post_agent_dirty_worktree_cleanup(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    dir: &str,
    pre_task_dirty_paths: Option<&[String]>,
) -> Result<DirtyWorktreeAction> {
    if args.read_only {
        return read_only::settle(store, task_id, args, dir, pre_task_dirty_paths);
    }
    if args.audit_report_mode {
        return Ok(DirtyWorktreeAction::Continue);
    }

    let task = store
        .get_task(task_id.as_str())?
        .ok_or_else(|| anyhow::anyhow!("Task {task_id} disappeared before settlement"))?;
    let Some(worktree) = task.worktree_path.as_deref() else {
        return preserve_shared_checkout(store, task_id, dir);
    };
    // Never commit into a different checkout because a stale path reached settlement.
    anyhow::ensure!(
        Path::new(worktree).canonicalize()? == Path::new(dir).canonicalize()?,
        "Task worktree does not match settlement directory: {worktree} vs {dir}"
    );
    let start_sha = task.start_sha;

    rescue_dirty_worktree(
        store,
        task_id,
        dir,
        pre_task_dirty_paths,
        start_sha.as_deref(),
    );

    settle_remaining_dirty_files(store, task_id, args, dir, pre_task_dirty_paths).await
}

pub(crate) fn final_dirty_assertion(
    store: &Store,
    task_id: &TaskId,
    dir: &str,
    read_only: bool,
    baseline: Option<&[String]>,
) -> Result<bool> {
    if read_only {
        return Ok(false);
    }

    let mut dirty_files = match worktree_status_lines(dir) {
        Ok(lines) => lines,
        Err(err) => {
            aid_warn!("[aid] final dirty assertion skipped for {dir}: {err}");
            return Ok(false);
        }
    };
    if let Some(baseline) = baseline {
        let baseline_paths = crate::workspace::worktree::extract_baseline_paths(baseline);
        dirty_files.retain(|line| {
            crate::workspace::worktree::extract_baseline_path(line)
                .map(|path| !crate::workspace::worktree::baseline_contains(&baseline_paths, &path))
                .unwrap_or(true)
        });
    }
    if dirty_files.is_empty() {
        return Ok(false);
    }

    let files = dirty_files.join(", ");
    let detail = format!(
        "FAIL: agent left uncommitted changes after rescue and retry — aborting to prevent data loss. Files: {files}"
    );
    aid_warn!("[aid] {detail}");
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail,
        metadata: None,
    })?;
    crate::task::lifecycle::mark_failed(store, task_id)?;
    Ok(true)
}

async fn maybe_retry_uncommitted(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    dir: &str,
) -> Result<Option<TaskId>> {
    if args.retry == 0
        && args.parent_task_id.is_some()
        && args
            .prompt
            .starts_with("You have uncommitted changes in your worktree.")
    {
        return Ok(None);
    }

    let Some(task) = store.get_task(task_id.as_str())? else {
        return Ok(None);
    };
    if task.status != TaskStatus::Done {
        return Ok(None);
    }

    let Some(wt_path) = task.worktree_path.as_deref() else {
        return Ok(None);
    };
    if !Path::new(wt_path).exists() {
        return Ok(None);
    }

    let dirty_files = worktree_status_lines(dir).unwrap_or_default().join(", ");
    aid_warn!("[aid] retry: uncommitted changes remain in {dir} — {dirty_files}");
    let _ = store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: format!("Retrying uncommitted changes: {dirty_files}"),
        metadata: None,
    });

    let mut retry_args = build_uncommitted_retry_args(args, &task, task_id, dir)?;
    if task.agent.supports_session_resume() {
        retry_args.session_id = task.agent_session_id.clone();
    }

    Box::pin(run(store.clone(), retry_args)).await.map(Some)
}

fn build_uncommitted_retry_args(
    args: &RunArgs,
    task: &Task,
    task_id: &TaskId,
    dir: &str,
) -> Result<RunArgs> {
    let mut retry_args = args.clone();
    retry_args.prompt = format!(
        "You have uncommitted changes in your worktree. Please review them with `git status` and `git diff`, then commit ALL your changes with a descriptive message. Do not leave any files uncommitted.\n\n[Original task for context]\n{}",
        task.prompt
    );
    retry_args.retry = 0;
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    if retry_args.model.is_none() {
        retry_args.model = task.requested_model.clone();
    }
    retry_args.verify = task.verify.clone().or_else(|| args.verify.clone());
    retry_args.read_only = false;
    retry_args.background = false;
    retry_args.judge = None;
    retry_args.peer_review = None;
    retry_args.checklist = Vec::new();
    let (retry_dir, worktree) = retry_target(task)?;
    retry_args.dir = Some(dir.to_string())
        .or(retry_dir)
        .or_else(|| retry_args.dir.clone());
    retry_args.worktree = worktree.or_else(|| retry_args.worktree.clone());
    Ok(retry_args)
}

fn rescue_files_summary(outcome: &crate::workspace::commit::RescueOutcome) -> String {
    let mut files = Vec::new();
    files.extend(
        outcome
            .untracked
            .iter()
            .map(|path| format!("untracked:{path}")),
    );
    files.extend(
        outcome
            .modified
            .iter()
            .map(|path| format!("modified:{path}")),
    );
    files.join(", ")
}

/// Only what the agent left. aid's own bookkeeping is filtered out: this feeds both
/// the retry decision and the data-loss assertion, and aid removing its own
/// `.aid-lock` on the way out is not the agent leaving work behind.
fn worktree_status_lines(dir: &str) -> Result<Vec<String>> {
    Ok(crate::workspace::worktree::capture_worktree_snapshot(Path::new(dir))?.agent_status_lines())
}

/// Shared checkouts remain dirty for the principal to review and commit. A
/// checkpoint is recovery evidence, not acceptance or an automatic retry request.

#[path = "dirty/settlement.rs"]
mod settlement;
use settlement::{preserve_shared_checkout, rescue_dirty_worktree, settle_remaining_dirty_files};
