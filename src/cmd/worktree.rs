// Handler for `aid worktree` — explicit worktree lifecycle management.
// Lets callers create, list, and remove worktrees outside of task dispatch.
// Deps: crate::worktree

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::Path;
use std::time::SystemTime;

const STALE_WORKTREE_AGE_SECS: u64 = 24 * 60 * 60;

#[derive(Serialize)]
struct AidWorktreeEntry {
    path: String,
    branch: String,
    active: bool,
    lock_pid: Option<u32>,
    lock_task_id: Option<String>,
    modified_age_secs: u64,
}

struct WorktreeLock {
    pid: u32,
    task_id: Option<String>,
}

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
pub fn list(repo: Option<&str>, json: bool, active_only: bool) -> Result<()> {
    let repo_dir = repo.unwrap_or(".");
    if json {
        println!("{}", list_json(Some(repo_dir), active_only)?);
        return Ok(());
    }
    let entries = filtered_worktree_entries(repo_dir, active_only)?;
    let mut count = 0;
    for entry in entries {
        println!("{:<50} {}", entry.path, entry.branch);
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
    let mut locks_cleared = 0usize;
    let mut pruned = 0usize;
    for entry in entries {
        let path = Path::new(&entry.path);
        let was_stale = is_stale_worktree_path(path);
        if let Some(lock) = read_worktree_lock(path) {
            if crate::worktree::process_alive_check(lock.pid) {
                if was_stale {
                    let task = lock.task_id.as_deref().unwrap_or("unknown");
                    aid_warn!(
                        "[aid] Skipping prune: {} has active task {} (pid {})",
                        entry.path,
                        task,
                        lock.pid
                    );
                }
                continue;
            }
            let task = lock.task_id.as_deref().unwrap_or("unknown");
            println!(
                "[aid] Cleared stale lock in {} (task={}, pid={} dead)",
                entry.path,
                task,
                lock.pid
            );
            let _ = std::fs::remove_file(path.join(".aid-lock"));
            locks_cleared += 1;
        }
        if !was_stale {
            continue;
        }
        match super::merge::remove_worktree(repo_dir, &entry.path) {
            Ok(()) => {
                println!("[aid] Pruned stale worktree: {}", entry.path);
                pruned += 1;
            }
            Err(err) => aid_warn!("[aid] Failed to prune {}: {err}", entry.path),
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
        .map(|entry| entry.path)
        .filter(|path| should_prune_worktree(path))
        .collect())
}

fn list_json(repo: Option<&str>, active_only: bool) -> Result<String> {
    let entries = filtered_worktree_entries(repo.unwrap_or("."), active_only)?;
    serde_json::to_string_pretty(&entries).map_err(Into::into)
}

fn filtered_worktree_entries(repo_dir: &str, active_only: bool) -> Result<Vec<AidWorktreeEntry>> {
    Ok(aid_worktree_entries(repo_dir)?
        .into_iter()
        .filter(|entry| !active_only || entry.active)
        .collect())
}

fn aid_worktree_entries(repo_dir: &str) -> Result<Vec<AidWorktreeEntry>> {
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
    entries: &mut Vec<AidWorktreeEntry>,
    current_path: &mut String,
    current_branch: &mut String,
) {
    if is_aid_worktree(current_path) && !current_path.is_empty() {
        entries.push(aid_worktree_entry(current_path, current_branch));
    }
    current_path.clear();
    current_branch.clear();
}

fn aid_worktree_entry(path: &str, branch: &str) -> AidWorktreeEntry {
    let lock = live_worktree_lock(Path::new(path));
    AidWorktreeEntry {
        path: path.to_string(),
        branch: branch.to_string(),
        active: lock.is_some(),
        lock_pid: lock.as_ref().map(|lock| lock.pid),
        lock_task_id: lock.and_then(|lock| lock.task_id),
        modified_age_secs: modified_age_secs(Path::new(path)),
    }
}

fn read_worktree_lock(wt_path: &Path) -> Option<WorktreeLock> {
    let content = std::fs::read_to_string(wt_path.join(".aid-lock")).ok()?;
    let mut pid = None;
    let mut task_id = None;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("pid=") {
            pid = value.trim().parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("task=") {
            task_id = Some(value.trim().to_string());
        }
    }
    Some(WorktreeLock { pid: pid?, task_id })
}

fn live_worktree_lock(wt_path: &Path) -> Option<WorktreeLock> {
    let lock = read_worktree_lock(wt_path)?;
    crate::worktree::process_alive_check(lock.pid).then_some(lock)
}

fn should_prune_worktree(wt_path: &str) -> bool {
    if live_worktree_lock(Path::new(wt_path)).is_some() {
        return false;
    }
    is_stale_worktree_path(Path::new(wt_path))
}

fn is_stale_worktree_path(wt_path: &Path) -> bool {
    std::fs::metadata(wt_path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .map(is_stale_worktree_time)
        .unwrap_or(true)
}

fn modified_age_secs(wt_path: &Path) -> u64 {
    std::fs::metadata(wt_path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|age| age.as_secs())
        .unwrap_or(0)
}

fn is_stale_worktree_time(modified: SystemTime) -> bool {
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age.as_secs() > STALE_WORKTREE_AGE_SECS)
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "worktree/tests.rs"]
mod tests;
