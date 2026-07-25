// Retry target selection for run follow-up dispatches.
// Exports retry_target and apply_retry_target.
// Deps: RunArgs plus persisted Task worktree/repo fields.

use anyhow::Result;
use std::path::Path;

use crate::cmd::run::RunArgs;
use crate::types::Task;

pub(crate) fn retry_target(task: &Task) -> Result<(Option<String>, Option<String>)> {
    match task.worktree_path.as_ref() {
        Some(path) if Path::new(path).is_dir() => {
            crate::worktree::ensure_consumed_worktree_path_is_isolated(
                task.repo_path.as_deref(),
                path,
                &format!("recorded worktree path for task {}", task.id),
            )?;
            Ok((Some(path.clone()), task.worktree_branch.clone()))
        }
        Some(_) => Ok((None, task.worktree_branch.clone())),
        None => Ok((None, None)),
    }
}

pub(crate) fn apply_retry_target(task: &Task, retry_args: &mut RunArgs) -> Result<()> {
    // Anchor the retry to the task's repository. Without this the dir below (a worktree
    // path) would be resolved as its own repo root, and the isolation guard would then see
    // the task's own branch checked out in what it takes to be the caller checkout.
    if retry_args.repo.is_none() {
        retry_args.repo = task.repo_path.clone();
    }
    let (dir, worktree) = retry_target(task)?;
    if dir.is_some() && retry_args.repo.is_none() {
        // Without a repo anchor the worktree dir below would be resolved as its own repo
        // root, and the isolation guard would then reject a legitimate re-entry.
        anyhow::bail!(
            "cannot retry task {}: legacy task metadata has no repo_path, so its live worktree cannot be anchored to a repository",
            task.id
        );
    }
    if let Some(dir) = dir {
        retry_args.dir = Some(dir);
        retry_args.worktree = worktree
            .or_else(|| task.worktree_branch.clone())
            .or_else(|| retry_args.worktree.clone());
        return Ok(());
    }
    retry_args.dir = existing_retry_dir(task, retry_args)
        .or_else(|| repo_dir(task));
    retry_args.worktree = worktree
        .or_else(|| task.worktree_branch.clone())
        .or_else(|| retry_args.worktree.clone());
    if retry_args.dir.is_none() {
        anyhow::bail!(
            "cannot retry task {}: no usable worktree path, retry dir, or repo path",
            task.id
        );
    }
    Ok(())
}

fn repo_dir(task: &Task) -> Option<String> {
    let repo_path = task.repo_path.as_ref()?;
    Path::new(repo_path).is_dir().then(|| repo_path.clone())
}

fn existing_retry_dir(task: &Task, retry_args: &RunArgs) -> Option<String> {
    let dir = retry_args.dir.as_ref()?;
    let dir_path = Path::new(dir);
    if !dir_path.is_dir() {
        return None;
    }
    let Some(repo_path) = task.repo_path.as_deref() else {
        return Some(dir.clone());
    };
    let repo_path = Path::new(repo_path);
    let Ok(dir_path) = dir_path.canonicalize() else {
        return None;
    };
    let Ok(repo_path) = repo_path.canonicalize() else {
        return None;
    };
    dir_path.starts_with(repo_path).then(|| dir.clone())
}
