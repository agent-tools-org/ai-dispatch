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

pub fn create_worktree(repo_dir: &Path, branch: &str, base_branch: Option<&str>) -> Result<WorktreeInfo> {
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
        .args(base_branch)
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
    Ok(WorktreeInfo {
        path: wt_path,
        branch: branch.to_string(),
    })
}

pub fn branch_has_commits_ahead_of_main(repo_dir: &Path, branch: &str) -> Result<bool> {
    validate_git_repo(repo_dir)?;
    let status = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-parse", "--verify", branch])
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
    Ok(String::from_utf8_lossy(&out.stdout).trim().parse::<u32>().unwrap_or(0) > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    fn git(repo_dir: &Path, args: &[&str]) {
        let repo_dir = repo_dir.to_string_lossy().to_string();
        assert!(Command::new("git").args(["-C", repo_dir.as_str()]).args(args).status().unwrap().success());
    }

    fn unique_branch(prefix: &str) -> String {
        format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        )
    }

    #[test]
    fn validate_git_repo_fails_on_nonrepo() {
        assert!(validate_git_repo(Path::new("/tmp")).is_err());
    }

    #[test]
    fn validate_git_repo_succeeds_on_real_repo() {
        assert!(validate_git_repo(Path::new(env!("CARGO_MANIFEST_DIR"))).is_ok());
    }

    #[test]
    fn create_worktree_with_base_branch_inherits_base_content() {
        let repo = TempDir::new().unwrap();
        git(repo.path(), &["init", "-b", "main"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "Test User"]);
        std::fs::write(repo.path().join("base.txt"), "main\n").unwrap();
        git(repo.path(), &["add", "base.txt"]);
        git(repo.path(), &["commit", "-m", "init"]);

        let base_branch = unique_branch("base");
        git(repo.path(), &["checkout", "-b", base_branch.as_str()]);
        std::fs::write(repo.path().join("inherited.txt"), "from base\n").unwrap();
        git(repo.path(), &["add", "inherited.txt"]);
        git(repo.path(), &["commit", "-m", "base"]);
        git(repo.path(), &["checkout", "main"]);

        let retry_branch = unique_branch("retry");
        let info = create_worktree(repo.path(), retry_branch.as_str(), Some(base_branch.as_str())).unwrap();

        assert_eq!(std::fs::read_to_string(info.path.join("inherited.txt")).unwrap(), "from base\n");
        git(repo.path(), &["worktree", "remove", "--force", &info.path.to_string_lossy()]);
    }
}
