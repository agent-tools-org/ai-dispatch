// Handler for `aid worktree` — explicit worktree lifecycle management.
// Lets callers create, list, and remove worktrees outside of task dispatch.
// Deps: crate::worktree

use anyhow::Result;
use std::path::Path;

/// Check if a path is an aid-managed worktree (handles macOS /tmp → /private/tmp).
fn is_aid_worktree(path: &str) -> bool {
    path.starts_with("/tmp/aid-wt-") || path.starts_with("/private/tmp/aid-wt-")
}

/// Create a worktree, print its path to stdout for capture.
pub fn create(branch: &str, base: Option<&str>, repo: Option<&str>) -> Result<()> {
    let repo_dir = repo.unwrap_or(".");
    let repo_path = Path::new(repo_dir).canonicalize()?;
    let info = crate::worktree::create_worktree(&repo_path, branch, base)?;
    println!("{}", info.path.display());
    eprintln!(
        "[aid] Created worktree on branch '{}' at {}",
        info.branch,
        info.path.display()
    );
    Ok(())
}

/// List all aid-managed worktrees (/tmp/aid-wt-*).
pub fn list(repo: Option<&str>) -> Result<()> {
    let repo_dir = repo.unwrap_or(".");
    let output = std::process::Command::new("git")
        .args(["-C", repo_dir, "worktree", "list", "--porcelain"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("git worktree list failed");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut count = 0;
    let mut current_path = String::new();
    let mut current_branch = String::new();
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = path.to_string();
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = branch.to_string();
        } else if line.is_empty() && is_aid_worktree(&current_path) {
            println!("{:<50} {}", current_path, current_branch);
            count += 1;
            current_path.clear();
            current_branch.clear();
        } else if line.is_empty() {
            current_path.clear();
            current_branch.clear();
        }
    }
    if is_aid_worktree(&current_path) && !current_path.is_empty() {
        println!("{:<50} {}", current_path, current_branch);
        count += 1;
    }
    if count == 0 {
        println!("No active aid worktrees.");
    }
    Ok(())
}

/// Remove a worktree by branch name and prune stale references.
pub fn remove(branch: &str, repo: Option<&str>) -> Result<()> {
    let wt_path = format!("/tmp/aid-wt-{branch}");
    if !Path::new(&wt_path).exists() {
        anyhow::bail!("Worktree not found: {wt_path}");
    }
    let repo_dir = repo.unwrap_or(".");
    super::merge::remove_worktree(repo_dir, &wt_path);
    Ok(())
}
