// Extracted settlement phases.
// Deps: parent module types and helpers.
use super::*;

pub(super) fn preserve_shared_checkout(
    store: &Store,
    task_id: &TaskId,
    dir: &str,
) -> Result<DirtyWorktreeAction> {
    match crate::workspace::commit::preserve_shared_checkout(Path::new(dir), task_id.as_str()) {
        Ok(Some(checkpoint)) => {
            let detail = format!(
                "Shared checkout preserved at {} ({}). HEAD, index and files were left unchanged; \
                 this recovery snapshot can include pre-existing user edits. Inspect with git show {}.",
                checkpoint.reference, checkpoint.commit, checkpoint.reference,
            );
            aid_info!("[aid] {detail}");
            store.insert_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: chrono::Local::now(),
                event_kind: EventKind::Milestone,
                detail,
                metadata: Some(serde_json::json!({
                    "recovery_ref": checkpoint.reference,
                    "recovery_commit": checkpoint.commit,
                    "shared_checkout": true,
                })),
            })?;
        }
        Ok(None) => {}
        Err(error) => {
            let detail =
                format!("Shared checkout preservation failed; original files retained: {error:#}");
            aid_warn!("[aid] {detail}");
            store.insert_agent_event(&TaskEvent {
                task_id: task_id.clone(),
                timestamp: chrono::Local::now(),
                event_kind: EventKind::Error,
                detail,
                metadata: None,
            })?;
            crate::task::lifecycle::mark_failed(store, task_id)?;
            return Ok(DirtyWorktreeAction::Failed);
        }
    }
    Ok(DirtyWorktreeAction::Continue)
}

pub(super) fn rescue_dirty_worktree(
    store: &Store,
    task_id: &TaskId,
    dir: &str,
    pre_task_dirty_paths: Option<&[String]>,
    start_sha: Option<&str>,
) {
    match crate::workspace::commit::rescue_dirty_worktree_with_baseline(
        dir,
        task_id.as_str(),
        pre_task_dirty_paths,
        start_sha,
    ) {
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
}

pub(super) async fn settle_remaining_dirty_files(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    dir: &str,
    pre_task_dirty_paths: Option<&[String]>,
) -> Result<DirtyWorktreeAction> {
    let mut retry_id = None;
    if crate::workspace::commit::has_uncommitted_changes(dir).unwrap_or(false) {
        retry_id = maybe_retry_uncommitted(store, task_id, args, dir).await?;
    }
    if final_dirty_assertion(
        store.as_ref(),
        task_id,
        dir,
        args.read_only,
        pre_task_dirty_paths,
    )? {
        return Ok(DirtyWorktreeAction::Failed);
    }

    Ok(match retry_id {
        Some(task_id) => DirtyWorktreeAction::Retry(task_id),
        None => DirtyWorktreeAction::Continue,
    })
}
