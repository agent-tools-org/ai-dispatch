// Git worktree management: create, remove, and diff isolated worktrees.
// Used by `aid run --worktree` and `aid batch` for parallel conflict-free dispatch.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::sanitize;
#[path = "worktree/reconcile.rs"]
mod reconcile;

const AID_BRANCH_PREFIXES: &[&str] = &["feat/", "fix/", "docs/", "chore/", "test/", "refactor/"];

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

fn is_valid_git_worktree(path: &Path) -> Result<bool> {
    let status = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--git-dir"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run git rev-parse")?;
    Ok(status.success())
}

fn canonical_worktree_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn prune_worktrees(repo_dir: &Path) -> Result<()> {
    let prune_status = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "worktree", "prune"])
        .status()
        .context("Failed to run git worktree prune")?;
    anyhow::ensure!(prune_status.success(), "git worktree prune failed");
    Ok(())
}

fn stale_worktree_warning(path: &Path) {
    aid_warn!(
        "[aid] Warning: Cleaned stale worktree at {}, re-creating",
        path.display()
    );
}

fn remove_stale_worktree_dir(path: &Path) -> Result<()> {
    std::fs::remove_dir_all(path)
        .with_context(|| format!("Failed to remove stale worktree at {}", path.display()))
}

fn worktree_create_error(path: &Path, branch: &str, reason: impl std::fmt::Display) -> anyhow::Error {
    anyhow!(
        "Failed to create worktree at {} for branch {}: {}. Try: aid worktree prune",
        path.display(),
        branch,
        reason
    )
}

fn worktree_add_reason(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.lines().next().unwrap_or(stderr).to_string();
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stdout = stdout.trim();
    if !stdout.is_empty() {
        return stdout.lines().next().unwrap_or(stdout).to_string();
    }
    "git worktree add failed".to_string()
}

/// Sync repo-backed context files into the worktree when they are missing there.
pub fn sync_context_files_into_worktree(repo_dir: &Path, wt_path: &Path, context_files: &[String]) -> Vec<String> {
    let mut synced = Vec::new();
    for file in context_files {
        let file_path = Path::new(file);
        let rel_path = if let Ok(stripped) = file_path.strip_prefix(repo_dir) {
            stripped.to_path_buf()
        } else if file_path.is_relative() {
            PathBuf::from(file)
        } else {
            continue;
        };
        let wt_file = wt_path.join(&rel_path);
        let repo_file = repo_dir.join(&rel_path);
        if wt_file.exists() || !repo_file.exists() { continue; }
        if let Some(parent) = wt_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(&repo_file, &wt_file).is_ok() { synced.push(rel_path.display().to_string()); }
    }
    synced
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
        if let (Some(path), Some(branch_line)) =
            (current_path.as_ref(), line.strip_prefix("branch "))
        {
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

fn is_aid_managed_branch(branch: &str) -> bool {
    AID_BRANCH_PREFIXES
        .iter()
        .any(|prefix| branch.starts_with(prefix))
}

fn local_branch_exists(repo_dir: &Path, branch: &str) -> Result<bool> {
    let status = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}"),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run git rev-parse")?;
    Ok(status.success())
}

pub fn create_worktree(
    repo_dir: &Path,
    branch: &str,
    base_branch: Option<&str>,
) -> Result<WorktreeInfo> {
    sanitize::validate_branch_name(branch)?;
    if let Some(base_branch) = base_branch {
        sanitize::validate_branch_name(base_branch)?;
    }
    validate_git_repo(repo_dir)?;
    let wt_path = PathBuf::from(format!("/tmp/aid-wt-{branch}"));

    if wt_path.exists() {
        // Reject symlinks to prevent symlink-following attacks on /tmp paths
        if wt_path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            anyhow::bail!(
                "Worktree path {} is a symlink — refusing to use for safety",
                wt_path.display()
            );
        }
        let expected_path = canonical_worktree_path(&wt_path);
        if existing_worktree_path(repo_dir, branch)?
            .is_some_and(|path| canonical_worktree_path(&path) != expected_path)
        {
            prune_worktrees(repo_dir)?;
        }
        if is_valid_git_worktree(&wt_path)? {
            if let Some(existing_path) = existing_worktree_path(repo_dir, branch)? {
                if existing_path.exists()
                    && canonical_worktree_path(&existing_path) != expected_path
                {
                    reconcile::maybe_refresh_existing_worktree(
                        repo_dir,
                        &existing_path,
                        branch,
                        base_branch,
                    )?;
                    sync_cargo_lock(repo_dir, &existing_path);
                    return Ok(WorktreeInfo {
                        path: existing_path,
                        branch: branch.to_string(),
                    });
                }
            }
            reconcile::maybe_refresh_existing_worktree(repo_dir, &wt_path, branch, base_branch)?;
            sync_cargo_lock(repo_dir, &wt_path);
            return Ok(WorktreeInfo {
                path: wt_path,
                branch: branch.to_string(),
            });
        }

        stale_worktree_warning(&wt_path);
        remove_stale_worktree_dir(&wt_path)?;
    }

    // Try new branch first
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "add", &wt_path.to_string_lossy(), "-b", branch])
        .args(base_branch)
        .output()
        .map_err(|err| worktree_create_error(&wt_path, branch, format!("failed to run git worktree add: {err}")))?;

    if out.status.success() {
        sync_cargo_lock(repo_dir, &wt_path);
        return Ok(WorktreeInfo {
            path: wt_path,
            branch: branch.to_string(),
        });
    }

    if let Some(existing_path) = existing_worktree_path(repo_dir, branch)? {
        if existing_path.exists() {
            reconcile::maybe_refresh_existing_worktree(repo_dir, &existing_path, branch, base_branch)?;
            sync_cargo_lock(repo_dir, &existing_path);
            return Ok(WorktreeInfo {
                path: existing_path,
                branch: branch.to_string(),
            });
        }

        prune_worktrees(repo_dir)?;
    }

    // Fallback: existing branch — reset it to HEAD first to avoid stale checkout
    let branch_exists = local_branch_exists(repo_dir, branch)?;
    if !is_aid_managed_branch(branch) {
        if branch_exists {
            aid_warn!(
                "[aid] Warning: refusing to force-reset existing non aid-managed branch '{branch}'"
            );
        }
        anyhow::bail!(
            "Refusing to force-reset branch '{branch}' — branch must start with one of: {}",
            AID_BRANCH_PREFIXES.join(", ")
        );
    }
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
        .map_err(|err| worktree_create_error(&wt_path, branch, format!("failed to run git worktree add: {err}")))?;
    if !out.status.success() {
        return Err(worktree_create_error(
            &wt_path,
            branch,
            worktree_add_reason(&out),
        ));
    }
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

/// Returns the files touched by the agent's commits in `wt_path`.
pub fn worktree_changed_files(wt_path: &Path) -> Result<Vec<String>> {
    let repo = wt_path.to_string_lossy().to_string();
    let range = if commits_ahead_of_main(&repo).unwrap_or(0) > 1 {
        "main..HEAD"
    } else {
        "HEAD~1..HEAD"
    };
    let out = Command::new("git")
        .args(["-C", &repo, "diff", "--name-only", range])
        .output()
        .context("Failed to run git diff --name-only")?;
    anyhow::ensure!(
        out.status.success(),
        "git diff failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let files = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect();
    Ok(files)
}

// --- Worktree lock file: prevents concurrent agent access ---

const LOCK_FILENAME: &str = ".aid-lock";

/// Check if a worktree is locked by an active process. Returns the holder task ID if locked.
pub fn check_worktree_lock(wt_path: &Path) -> Option<String> {
    let lock_path = wt_path.join(LOCK_FILENAME);
    let content = std::fs::read_to_string(&lock_path).ok()?;
    let mut task_id = None;
    let mut pid = None;
    for line in content.lines() {
        if let Some(t) = line.strip_prefix("task=") { task_id = Some(t.trim().to_string()); }
        if let Some(p) = line.strip_prefix("pid=") { pid = p.trim().parse::<u32>().ok(); }
    }
    // Stale lock: process is dead
    if let Some(p) = pid {
        if !process_alive(p) {
            let _ = std::fs::remove_file(&lock_path);
            return None;
        }
    }
    task_id
}

/// Write a lock file claiming this worktree for a task.
pub fn write_worktree_lock(wt_path: &Path, task_id: &str) {
    let lock_path = wt_path.join(LOCK_FILENAME);
    let content = format!("task={task_id}\npid={}\n", std::process::id());
    let _ = std::fs::write(&lock_path, content);
}

/// Remove the lock file when a task finishes using the worktree.
pub fn clear_worktree_lock(wt_path: &Path) {
    let _ = std::fs::remove_file(wt_path.join(LOCK_FILENAME));
}

/// Check if a process is alive (used by lock cleanup in cmd::worktree).
pub fn process_alive_check(pid: u32) -> bool { process_alive(pid) }

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool { false }

fn commits_ahead_of_main(repo: &str) -> Option<u32> {
    let out = Command::new("git")
        .args(["-C", repo, "rev-list", "--count", "main..HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let output = String::from_utf8_lossy(&out.stdout);
    let trimmed = output.trim();
    trimmed.parse::<u32>().ok()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "worktree/stale_tests.rs"]
mod stale_tests;
