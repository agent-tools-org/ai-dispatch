// Dirty-worktree enforcement shared by foreground and background runs.
// Exports rescue/retry/final-assertion helpers plus the post-run action enum.
// Deps: commit helpers, run retry flow, store, and task types.

use anyhow::Result;
use std::{path::Path, sync::Arc};

use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};

use super::{RunArgs, retry_target, run};

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
) -> Result<DirtyWorktreeAction> {
    if args.read_only {
        return Ok(DirtyWorktreeAction::Continue);
    }

    match crate::commit::rescue_dirty_worktree(dir, task_id.as_str()) {
        Ok(outcome) if !outcome.staged.is_empty() => {
            let files_list = rescue_files_summary(&outcome);
            aid_warn!(
                "[aid] rescue: staged {} file(s) in {} — {}",
                outcome.staged.len(),
                dir,
                files_list
            );
            let _ = store.insert_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: chrono::Local::now(),
                event_kind: EventKind::Milestone,
                detail: format!("Rescued {} file(s): {files_list}", outcome.staged.len()),
                metadata: None,
            });
            if let Some(error) = outcome.error {
                aid_warn!("[aid] rescue: failed to commit rescued files in {dir}: {error}");
            }
        }
        Ok(outcome) if outcome.error.is_some() => {
            aid_warn!(
                "[aid] rescue: dirty worktree rescue failed in {dir}: {}",
                outcome.error.unwrap_or_default()
            );
        }
        Err(err) => aid_warn!("[aid] rescue: dirty worktree rescue failed in {dir}: {err}"),
        _ => {}
    }

    let mut retry_id = None;
    if crate::commit::has_uncommitted_changes(dir).unwrap_or(false) {
        retry_id = maybe_retry_uncommitted(store, task_id, args, dir).await?;
    }
    if final_dirty_assertion(store.as_ref(), task_id, dir, args.read_only)? {
        return Ok(DirtyWorktreeAction::Failed);
    }

    Ok(match retry_id {
        Some(task_id) => DirtyWorktreeAction::Retry(task_id),
        None => DirtyWorktreeAction::Continue,
    })
}

pub(crate) fn final_dirty_assertion(
    store: &Store,
    task_id: &TaskId,
    dir: &str,
    read_only: bool,
) -> Result<bool> {
    if read_only {
        return Ok(false);
    }

    let dirty_files = match worktree_status_lines(dir) {
        Ok(lines) => lines,
        Err(err) => {
            aid_warn!("[aid] final dirty assertion skipped for {dir}: {err}");
            return Ok(false);
        }
    };
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
    store.update_task_status(task_id.as_str(), TaskStatus::Failed)?;
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

    let mut retry_args = build_uncommitted_retry_args(args, &task, task_id, dir);
    if task.agent == crate::types::AgentKind::OpenCode {
        retry_args.session_id = task.agent_session_id.clone();
    }

    Box::pin(run(store.clone(), retry_args)).await.map(Some)
}

fn build_uncommitted_retry_args(
    args: &RunArgs,
    task: &Task,
    task_id: &TaskId,
    dir: &str,
) -> RunArgs {
    let mut retry_args = args.clone();
    retry_args.prompt = format!(
        "You have uncommitted changes in your worktree. Please review them with `git status` and `git diff`, then commit ALL your changes with a descriptive message. Do not leave any files uncommitted.\n\n[Original task for context]\n{}",
        task.prompt
    );
    retry_args.retry = 0;
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    retry_args.model = task.model.clone().or_else(|| retry_args.model.clone());
    retry_args.verify = None;
    retry_args.read_only = false;
    retry_args.background = false;
    retry_args.judge = None;
    retry_args.peer_review = None;
    retry_args.checklist = Vec::new();
    let (retry_dir, worktree) = retry_target(task);
    retry_args.dir = Some(dir.to_string())
        .or(retry_dir)
        .or_else(|| retry_args.dir.clone());
    retry_args.worktree = worktree.or_else(|| retry_args.worktree.clone());
    retry_args
}

fn rescue_files_summary(outcome: &crate::commit::RescueOutcome) -> String {
    let mut files = Vec::new();
    files.extend(outcome.untracked.iter().map(|path| format!("untracked:{path}")));
    files.extend(outcome.modified.iter().map(|path| format!("modified:{path}")));
    files.join(", ")
}

fn worktree_status_lines(dir: &str) -> Result<Vec<String>> {
    Ok(crate::worktree::capture_worktree_snapshot(Path::new(dir))?.status_lines)
}
