// Final branch resolution helpers for `aid merge`.
// Exports branch selection and drift confirmation before merging.
// Deps: anyhow and task metadata.

use anyhow::{Result, anyhow};

use crate::types::Task;

pub(super) fn merge_source_branch(task: &Task) -> Option<&str> {
    task.final_branch
        .as_deref()
        .or(task.worktree_branch.as_deref())
}

fn branch_drift(task: &Task) -> Option<(&str, &str)> {
    let original = task.worktree_branch.as_deref()?;
    let final_branch = task.final_branch.as_deref()?;
    (original != final_branch).then_some((original, final_branch))
}

pub(super) fn warn_branch_drift(task: &Task) {
    if let Some((original, final_branch)) = branch_drift(task) {
        aid_warn!(
            "[aid] Warning: task {} agent switched branch: {original} -> {final_branch}",
            task.id
        );
    }
}

pub(super) fn ensure_branch_drift_confirmed(task: &Task, force: bool) -> Result<()> {
    let Some((original, final_branch)) = branch_drift(task) else {
        return Ok(());
    };
    warn_branch_drift(task);
    if force {
        return Ok(());
    }
    aid_hint!("[aid] Re-run with --force to merge the final branch {final_branch}");
    Err(anyhow!(
        "Task {} final branch differs from dispatch branch ({original} -> {final_branch})",
        task.id
    ))
}
