// Linked-worktree validation helpers for aid-managed /tmp worktrees.
// Exports: canonical_worktree_path() and is_valid_git_worktree() to parent module.
// Deps: anyhow, std::path, std::process::Command.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn canonical_worktree_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn is_valid_git_worktree(repo_dir: &Path, wt_path: &Path) -> Result<bool> {
    if !git_rev_parse_succeeds(wt_path, "--git-dir")? {
        return Ok(false);
    }

    let Some(repo_common_dir) = git_common_dir(repo_dir)? else {
        return Ok(false);
    };
    let Some(wt_common_dir) = git_common_dir(wt_path)? else {
        return Ok(false);
    };
    if repo_common_dir != wt_common_dir {
        return Ok(false);
    }

    is_registered_worktree(repo_dir, wt_path)
}

fn git_rev_parse_succeeds(path: &Path, arg: &str) -> Result<bool> {
    let status = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", arg])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to run git rev-parse {arg}"))?;
    Ok(status.success())
}

fn git_common_dir(path: &Path) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--git-common-dir"])
        .output()
        .context("Failed to run git rev-parse --git-common-dir")?;
    if !output.status.success() {
        return Ok(None);
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Ok(None);
    }

    let resolved = resolve_git_path(path, Path::new(raw.as_str()));
    Ok(resolved.canonicalize().ok())
}

fn resolve_git_path(base_dir: &Path, git_path: &Path) -> PathBuf {
    if git_path.is_absolute() {
        git_path.to_path_buf()
    } else {
        base_dir.join(git_path)
    }
}

fn is_registered_worktree(repo_dir: &Path, wt_path: &Path) -> Result<bool> {
    let expected = wt_path.canonicalize().ok();
    let Some(expected) = expected else {
        return Ok(false);
    };

    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "worktree", "list", "--porcelain"])
        .output()
        .context("Failed to run git worktree list --porcelain")?;
    if !output.status.success() {
        return Ok(false);
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(path) = line.strip_prefix("worktree ") else {
            continue;
        };
        let listed = Path::new(path.trim()).canonicalize().ok();
        if listed.as_ref().is_some_and(|listed| listed == &expected) {
            return Ok(true);
        }
    }

    Ok(false)
}
