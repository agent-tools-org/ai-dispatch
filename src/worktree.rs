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

pub fn create_worktree(repo_dir: &Path, branch: &str) -> Result<WorktreeInfo> {
    validate_git_repo(repo_dir)?;
    let wt_path = PathBuf::from(format!("/tmp/aid-wt-{branch}"));

    if wt_path.exists() {
        return Ok(WorktreeInfo {
            path: wt_path,
            branch: branch.to_string(),
        });
    }

    // Try new branch first
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "add", &wt_path.to_string_lossy(), "-b", branch])
        .output()
        .context("Failed to run git worktree add")?;

    if out.status.success() {
        return Ok(WorktreeInfo {
            path: wt_path,
            branch: branch.to_string(),
        });
    }

    // Fallback: existing branch — reset it to HEAD first to avoid stale checkout
    let _ = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "branch", "-f", branch, "HEAD"])
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
    Ok(WorktreeInfo {
        path: wt_path,
        branch: branch.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_git_repo_fails_on_nonrepo() {
        assert!(validate_git_repo(Path::new("/tmp")).is_err());
    }

    #[test]
    fn validate_git_repo_succeeds_on_real_repo() {
        assert!(validate_git_repo(Path::new(env!("CARGO_MANIFEST_DIR"))).is_ok());
    }
}
