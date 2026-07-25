// Dispatch guards that enforce task isolation invariants before launch.
// Exports the worktree-task repo-root guard used by run preparation.
// Deps: Task metadata plus std path canonicalization.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::types::Task;

pub(super) fn ensure_worktree_task_not_repo_root(
    task: &Task,
    resolved_dir: Option<&str>,
    repo_path: Option<&str>,
) -> Result<()> {
    let Some(branch) = task.worktree_branch.as_deref() else {
        return Ok(());
    };
    let dir = resolve_launch_dir(task, branch, resolved_dir)?;
    let repo = resolve_repo_anchor(task, branch, repo_path, &dir)?;
    if !same_existing_path(&dir, &repo)? {
        return Ok(());
    }
    anyhow::bail!(
        "refusing to launch task {} for worktree branch {} in repository root {}; resolved working directory must be an isolated worktree",
        task.id,
        branch,
        dir.display()
    )
}

fn resolve_launch_dir(task: &Task, branch: &str, resolved_dir: Option<&str>) -> Result<PathBuf> {
    if let Some(dir) = resolved_dir {
        return Ok(PathBuf::from(dir));
    }
    std::env::current_dir().with_context(|| {
        format!(
            "refusing to launch task {} for worktree branch {}: could not resolve launch working directory",
            task.id, branch
        )
    })
}

fn resolve_repo_anchor(
    task: &Task,
    branch: &str,
    repo_path: Option<&str>,
    launch_dir: &Path,
) -> Result<PathBuf> {
    if let Some(repo) = repo_path.or(task.repo_path.as_deref()) {
        return Ok(PathBuf::from(repo));
    }
    if let Some(repo) = main_checkout_from_git_common_dir(launch_dir) {
        return Ok(repo);
    }
    if let Some(repo) = task
        .worktree_path
        .as_deref()
        .and_then(|path| main_checkout_from_git_common_dir(Path::new(path)))
    {
        return Ok(repo);
    }
    anyhow::bail!(
        "refusing to launch task {} for worktree branch {}: could not resolve repository root from repo_path, launch directory, or task worktree path",
        task.id,
        branch
    )
}

fn main_checkout_from_git_common_dir(path: &Path) -> Option<PathBuf> {
    if !path.is_dir() {
        return None;
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("worktree ").map(str::trim))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn same_existing_path(left: &Path, right: &Path) -> Result<bool> {
    if !left.is_dir() || !right.is_dir() {
        return Ok(false);
    }
    let left = left
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", left.display()))?;
    let right = right
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", right.display()))?;
    Ok(left == right)
}

#[cfg(test)] #[path = "run_dispatch_guard_tests.rs"] mod tests;
