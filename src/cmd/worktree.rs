// Handler for `aid worktree` — explicit worktree lifecycle management.
// Lets callers create, list, and remove worktrees outside of task dispatch.
// Deps: crate::worktree

use anyhow::{Context, Result};
use std::path::Path;
use std::time::SystemTime;

const STALE_WORKTREE_AGE_SECS: u64 = 24 * 60 * 60;

/// Check if a path is an aid-managed worktree (delegates to shared sandbox guard).
fn is_aid_worktree(path: &str) -> bool {
    super::merge::merge_git::is_safe_worktree_path(path)
}

/// Create a worktree, print its path to stdout for capture.
pub fn create(branch: &str, base: Option<&str>, repo: Option<&str>) -> Result<()> {
    let repo_dir = repo.unwrap_or(".");
    let repo_path = Path::new(repo_dir).canonicalize()?;
    let info = crate::worktree::create_worktree(&repo_path, branch, base)?;
    println!("{}", info.path.display());
    aid_info!(
        "[aid] Created worktree on branch '{}' at {}",
        info.branch,
        info.path.display()
    );
    Ok(())
}

/// List all aid-managed worktrees (/tmp/aid-wt-*).
pub fn list(repo: Option<&str>) -> Result<()> {
    let repo_dir = repo.unwrap_or(".");
    let mut count = 0;
    for (path, branch) in aid_worktree_entries(repo_dir)? {
        println!("{:<50} {}", path, branch);
        count += 1;
    }
    if count == 0 {
        println!("No active aid worktrees.");
    }
    Ok(())
}

/// Remove stale aid-managed worktrees older than 24 hours.
pub fn prune(repo: Option<&str>) -> Result<()> {
    let repo_dir = repo.unwrap_or(".");
    let mut pruned = 0usize;
    for path in stale_worktree_paths(repo_dir)? {
        match super::merge::remove_worktree(repo_dir, &path) {
            Ok(()) => {
                println!("[aid] Pruned stale worktree: {path}");
                pruned += 1;
            }
            Err(err) => aid_warn!("[aid] Failed to prune {path}: {err}"),
        }
    }
    if pruned == 0 {
        println!("[aid] No stale worktrees found");
    } else {
        println!("[aid] Pruned {pruned} stale worktree(s)");
    }
    Ok(())
}

pub(crate) fn stale_worktree_count(repo: Option<&str>) -> Result<usize> {
    Ok(stale_worktree_paths(repo.unwrap_or("."))?.len())
}

/// Remove a worktree by branch name and prune stale references.
pub fn remove(branch: &str, repo: Option<&str>) -> Result<()> {
    let wt_path = format!("/tmp/aid-wt-{branch}");
    if !Path::new(&wt_path).exists() {
        anyhow::bail!("Worktree not found: {wt_path}");
    }
    let repo_dir = repo.unwrap_or(".");
    super::merge::remove_worktree(repo_dir, &wt_path)?;
    Ok(())
}

fn stale_worktree_paths(repo_dir: &str) -> Result<Vec<String>> {
    Ok(aid_worktree_entries(repo_dir)?
        .into_iter()
        .map(|(path, _branch)| path)
        .filter(|path| should_prune_worktree(path))
        .collect())
}

fn aid_worktree_entries(repo_dir: &str) -> Result<Vec<(String, String)>> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_dir, "worktree", "list", "--porcelain"])
        .output()
        .context("Failed to list worktrees")?;
    if !output.status.success() {
        anyhow::bail!("git worktree list failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    let mut current_path = String::new();
    let mut current_branch = String::new();
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = path.to_string();
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            current_branch = branch.to_string();
        } else if line.is_empty() {
            push_aid_worktree_entry(&mut entries, &mut current_path, &mut current_branch);
        }
    }
    push_aid_worktree_entry(&mut entries, &mut current_path, &mut current_branch);
    Ok(entries)
}

fn push_aid_worktree_entry(
    entries: &mut Vec<(String, String)>,
    current_path: &mut String,
    current_branch: &mut String,
) {
    if is_aid_worktree(current_path) && !current_path.is_empty() {
        entries.push((current_path.clone(), current_branch.clone()));
    }
    current_path.clear();
    current_branch.clear();
}

fn should_prune_worktree(wt_path: &str) -> bool {
    std::fs::metadata(wt_path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .map(is_stale_worktree_time)
        .unwrap_or(true)
}

fn is_stale_worktree_time(modified: SystemTime) -> bool {
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age.as_secs() > STALE_WORKTREE_AGE_SECS)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::should_prune_worktree;
    use std::process::Command;

    #[test]
    fn should_prune_worktree_old_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("aid-wt-old");
        std::fs::create_dir(&path).expect("create dir");
        let status = Command::new("touch")
            .args(["-t", "202001010000"])
            .arg(&path)
            .status()
            .expect("touch status");
        assert!(status.success());
        assert!(should_prune_worktree(path.to_str().expect("utf8 path")));
    }

    #[test]
    fn should_prune_worktree_recent_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("aid-wt-recent");
        std::fs::create_dir(&path).expect("create dir");
        assert!(!should_prune_worktree(path.to_str().expect("utf8 path")));
    }
}
