// Handler for `aid worktree` — explicit worktree lifecycle management.
// Lets callers create, list, and remove worktrees outside of task dispatch.
// Deps: crate::worktree

use anyhow::{Context, Result};
use std::path::Path;
use std::time::SystemTime;

const STALE_WORKTREE_AGE_SECS: u64 = 24 * 60 * 60;

/// Check if a path is an aid-managed worktree (delegates to shared sandbox guard).
fn is_aid_worktree(path: &str) -> bool {
    crate::worktree::is_aid_managed_worktree_path(Path::new(path))
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

/// List all aid-managed worktrees (~/.aid/worktrees/* plus legacy /tmp/aid-wt-*).
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

/// Remove stale aid-managed worktrees older than 24 hours,
/// and clear lock files left by dead processes on all worktrees.
pub fn prune(repo: Option<&str>) -> Result<()> {
    let repo_dir = repo.unwrap_or(".");
    let entries = aid_worktree_entries(repo_dir)?;
    // First pass: clear stale locks on ALL worktrees (not just old ones)
    let mut locks_cleared = 0usize;
    for (path, _) in &entries {
        if clear_dead_lock(Path::new(path)) {
            locks_cleared += 1;
        }
    }
    // Second pass: remove worktrees older than 24h
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
    if pruned == 0 && locks_cleared == 0 {
        println!("[aid] No stale worktrees found");
    } else {
        if pruned > 0 { println!("[aid] Pruned {pruned} stale worktree(s)"); }
        if locks_cleared > 0 { println!("[aid] Cleared {locks_cleared} stale lock(s)"); }
    }
    Ok(())
}

pub(crate) fn stale_worktree_count(repo: Option<&str>) -> Result<usize> {
    Ok(stale_worktree_paths(repo.unwrap_or("."))?.len())
}

/// Remove a worktree by branch name and prune stale references.
pub fn remove(branch: &str, repo: Option<&str>) -> Result<()> {
    let repo_dir = repo.unwrap_or(".");
    let repo_path = Path::new(repo_dir).canonicalize()?;
    let mut wt_path = crate::worktree::aid_worktree_path(&repo_path, branch);
    if !wt_path.exists() {
        let legacy_path = Path::new("/tmp").join(format!("aid-wt-{branch}"));
        if legacy_path.exists() {
            wt_path = legacy_path;
        }
    }
    if !wt_path.exists() {
        anyhow::bail!("Worktree not found: {}", wt_path.display());
    }
    let wt_path = wt_path.to_string_lossy().to_string();
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

/// Clear a .aid-lock file if the holding process is dead. Returns true if cleared.
fn clear_dead_lock(wt_path: &Path) -> bool {
    let lock_path = wt_path.join(".aid-lock");
    let content = match std::fs::read_to_string(&lock_path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let pid = content.lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|p| p.trim().parse::<u32>().ok());
    let Some(pid) = pid else { return false };
    if crate::worktree::process_alive_check(pid) {
        return false;
    }
    let task = content.lines()
        .find_map(|line| line.strip_prefix("task="))
        .unwrap_or("unknown");
    println!("[aid] Cleared stale lock in {} (task={}, pid={} dead)", wt_path.display(), task, pid);
    let _ = std::fs::remove_file(&lock_path);
    true
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
