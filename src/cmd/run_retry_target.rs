// Retry target selection for run follow-up dispatches.
// Exports retry_target and apply_retry_target.
// Deps: RunArgs plus persisted Task worktree/repo fields.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

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
            ensure_live_worktree_branch(task, path)?;
            Ok((Some(path.clone()), task.worktree_branch.clone()))
        }
        Some(path) if task.worktree_branch.is_none() => {
            anyhow::bail!(
                "cannot retry task {}: recorded worktree path {} is missing and no worktree_branch was recorded; refusing to run in repo root",
                task.id,
                path
            )
        }
        Some(_) => Ok((None, task.worktree_branch.clone())),
        None => Ok((None, task.worktree_branch.clone())),
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
    if let Some(branch) = worktree.clone().or_else(|| task.worktree_branch.clone()) {
        let repo = recover_repo_dir(task, retry_args, &branch)?;
        ensure_branch_exists(&repo, &branch, task)?;
        retry_args.repo = Some(repo.clone());
        retry_args.dir = Some(repo);
        retry_args.worktree = Some(branch.clone());
        if retry_args.base_branch.is_none() {
            retry_args.base_branch = Some(branch);
        }
        return Ok(());
    }
    retry_args.dir = existing_retry_dir(task, retry_args).or_else(|| repo_dir(task));
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

fn recover_repo_dir(task: &Task, retry_args: &RunArgs, branch: &str) -> Result<String> {
    for candidate in [
        task.repo_path.as_deref(),
        retry_args.repo.as_deref(),
        retry_args.dir.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(repo) = git_root(candidate)? {
            return Ok(repo);
        }
    }
    anyhow::bail!(
        "cannot retry task {} on branch {}: no recoverable repo_path for missing worktree; refusing to run in repo root",
        task.id,
        branch
    )
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

fn git_root(path: &str) -> Result<Option<String>> {
    if !Path::new(path).is_dir() {
        return Ok(None);
    }
    let output = Command::new("git")
        .args(["-C", path, "rev-parse", "--show-toplevel"])
        .output()
        .with_context(|| format!("Failed to inspect git repo at {path}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_string()))
}

fn ensure_branch_exists(repo: &str, branch: &str, task: &Task) -> Result<()> {
    let status = Command::new("git")
        .args(["-C", repo, "rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to inspect branch {branch}"))?;
    if status.success() {
        return Ok(());
    }
    anyhow::bail!(
        "cannot retry task {} on branch {}: recorded worktree is missing and branch does not exist in {}; refusing to run in repo root",
        task.id,
        branch,
        repo
    )
}

fn ensure_live_worktree_branch(task: &Task, path: &str) -> Result<()> {
    let Some(branch) = task.worktree_branch.as_deref() else {
        return Ok(());
    };
    let current = current_branch(path)?;
    if current.as_deref() == Some(branch) {
        return Ok(());
    }
    let observed = current.as_deref().unwrap_or("(detached HEAD)");
    anyhow::bail!(
        "cannot retry task {}: recorded worktree path {} is on branch {} but expected {}; refusing to run in the wrong worktree",
        task.id,
        path,
        observed,
        branch
    )
}

fn current_branch(path: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["-C", path, "symbolic-ref", "--short", "-q", "HEAD"])
        .output()
        .with_context(|| format!("Failed to inspect worktree branch at {path}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!branch.is_empty()).then_some(branch))
}
