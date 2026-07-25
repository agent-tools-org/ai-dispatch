// Retry target selection for run follow-up dispatches.
// Exports retry_target and apply_retry_target.
// Deps: RunArgs plus persisted Task worktree/repo fields.

use anyhow::Result;
use std::path::Path;

use crate::cmd::run::RunArgs;
use crate::types::Task;

pub(crate) fn retry_target(task: &Task) -> (Option<String>, Option<String>) {
    match task.worktree_path.as_ref() {
        Some(path) if Path::new(path).is_dir() => (Some(path.clone()), None),
        Some(_) => (None, task.worktree_branch.clone()),
        None => (None, None),
    }
}

pub(crate) fn apply_retry_target(task: &Task, retry_args: &mut RunArgs) -> Result<()> {
    let (dir, worktree) = retry_target(task);
    if let Some(dir) = dir {
        retry_args.dir = Some(dir);
        retry_args.worktree = None;
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
