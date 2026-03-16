// Git worktree management: create, remove, and diff isolated worktrees.
// Used by `aid run --worktree` and `aid batch` for parallel conflict-free dispatch.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
}

pub fn validate_git_repo(path: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--git-dir"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run git")?;
    anyhow::ensure!(status.success(), "Not a git repository: {}", path.display());
    Ok(())
}

fn sync_cargo_lock(repo_dir: &Path, wt_path: &Path) {
    let src = repo_dir.join("Cargo.lock");
    let dst = wt_path.join("Cargo.lock");
    if src.exists() {
        let _ = std::fs::copy(&src, &dst);
    }
}

fn existing_worktree_path(repo_dir: &Path, branch: &str) -> Result<Option<PathBuf>> {
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "list", "--porcelain"])
        .output()
        .context("Failed to run git worktree list")?;
    anyhow::ensure!(
        out.status.success(),
        "git worktree list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut current_path = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path.trim()));
            continue;
        }
        if line.trim().is_empty() {
            current_path = None;
            continue;
        }
        if let (Some(path), Some(branch_line)) = (
            current_path.as_ref(),
            line.strip_prefix("branch "),
        ) {
            let branch_name = branch_line
                .trim()
                .strip_prefix("refs/heads/")
                .unwrap_or(branch_line.trim());
            if branch_name == branch {
                return Ok(Some(path.clone()));
            }
        }
    }

    Ok(None)
}

pub fn create_worktree(
    repo_dir: &Path,
    branch: &str,
    base_branch: Option<&str>,
) -> Result<WorktreeInfo> {
    validate_git_repo(repo_dir)?;
    let wt_path = PathBuf::from(format!("/tmp/aid-wt-{branch}"));

    if wt_path.exists() {
        sync_cargo_lock(repo_dir, &wt_path);
        return Ok(WorktreeInfo {
            path: wt_path,
            branch: branch.to_string(),
        });
    }

    // Try new branch first
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "add", &wt_path.to_string_lossy(), "-b", branch])
        .args(base_branch)
        .output()
        .context("Failed to run git worktree add")?;

    if out.status.success() {
        sync_cargo_lock(repo_dir, &wt_path);
        return Ok(WorktreeInfo {
            path: wt_path,
            branch: branch.to_string(),
        });
    }

    if let Some(existing_path) = existing_worktree_path(repo_dir, branch)? {
        if existing_path.exists() {
            sync_cargo_lock(repo_dir, &existing_path);
            return Ok(WorktreeInfo {
                path: existing_path,
                branch: branch.to_string(),
            });
        }

        let prune_status = Command::new("git")
            .args([
                "-C",
                &repo_dir.to_string_lossy(),
                "worktree",
                "prune",
            ])
            .status()
            .context("Failed to run git worktree prune")?;
        anyhow::ensure!(prune_status.success(), "git worktree prune failed");
    }

    // Fallback: existing branch — reset it to HEAD first to avoid stale checkout
    let _ = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "branch",
            "-f",
            branch,
            base_branch.unwrap_or("HEAD"),
        ])
        .output();
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "add", &wt_path.to_string_lossy(), branch])
        .output()
        .context("Failed to run git worktree add")?;

    anyhow::ensure!(
        out.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    sync_cargo_lock(repo_dir, &wt_path);
    Ok(WorktreeInfo {
        path: wt_path,
        branch: branch.to_string(),
    })
}

pub fn branch_has_commits_ahead_of_main(repo_dir: &Path, branch: &str) -> Result<bool> {
    validate_git_repo(repo_dir)?;
    let status = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "rev-parse",
            "--verify",
            branch,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run git")?;
    if !status.success() {
        return Ok(false);
    }

    let out = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "rev-list",
            "--count",
            &format!("main..{branch}"),
        ])
        .output()
        .context("Failed to run git rev-list")?;
    if !out.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u32>()
        .unwrap_or(0)
        > 0)
}

#[cfg(test)]
mod tests;
