// Git helpers for `aid merge`; exports merge, check, cleanup, and verify helpers.
// Deps: crate::cmd::merge_verify, std::{fs, path::Path, process::Command}.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

#[path = "merge_verify.rs"]
mod merge_verify;
pub(crate) use merge_verify::{run_post_merge_verify, run_verify_in_worktree};

pub(crate) fn resolve_repo_dir(repo_path: Option<&str>, worktree_path: Option<&str>) -> String {
    if let Some(repo) = repo_path {
        return repo.to_string();
    }
    if let Some(wt) = worktree_path
        && let Ok(out) = Command::new("git")
            .args(["-C", wt, "rev-parse", "--show-toplevel"])
            .output()
        && out.status.success()
    {
        let toplevel = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Ok(common) = Command::new("git")
            .args(["-C", wt, "rev-parse", "--git-common-dir"])
            .output()
            && common.status.success()
        {
            let common_dir =
                String::from_utf8_lossy(&common.stdout).trim().to_string();
            let common_path = Path::new(&common_dir);
            if let Some(parent) = common_path.parent()
                && parent.join(".git").exists()
            {
                return parent.to_string_lossy().to_string();
            }
        }
        return toplevel;
    }
    ".".to_string()
}

pub(crate) fn commits_ahead(repo_dir: &str, branch: &str) -> u32 {
    let out = Command::new("git")
        .args(["-C", repo_dir, "rev-list", "--count", &format!("HEAD..{branch}")])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0),
        _ => 0,
    }
}

pub(crate) fn auto_commit_uncommitted(wt_path: &str, branch: &str) -> bool {
    let status = Command::new("git")
        .args(["-C", wt_path, "status", "--porcelain"])
        .output();
    let has_changes = match status {
        Ok(o) if o.status.success() => !o.stdout.is_empty(),
        _ => return false,
    };
    if !has_changes {
        return false;
    }
    aid_info!("[aid] Worktree has uncommitted changes — auto-committing on {branch}");
    let _ = Command::new("git")
        .args(["-C", wt_path, "add", "-A"])
        .output();
    let out = Command::new("git")
        .args([
            "-C",
            wt_path,
            "commit",
            "-m",
            "chore: auto-commit agent changes before merge",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            aid_info!("[aid] Auto-committed uncommitted changes");
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            aid_warn!(
                "[aid] Warning: auto-commit failed: {}",
                stderr.lines().next().unwrap_or("")
            );
            false
        }
        Err(e) => { aid_warn!("[aid] Warning: auto-commit failed: {e}"); false }
    }
}

pub(crate) fn git_merge_branch(repo_dir: &str, branch: &str) -> MergeResult {
    let head_before = Command::new("git")
        .args(["-C", repo_dir, "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let stashed = stash_local_changes(repo_dir);

    let output = Command::new("git")
        .args(["-C", repo_dir, "merge", branch, "--no-edit"])
        .output();
    let merge_result = match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.contains("Already up to date") {
                MergeResult::AlreadyUpToDate
            } else {
                let head_after = Command::new("git")
                    .args(["-C", repo_dir, "rev-parse", "HEAD"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
                if head_before == head_after {
                    MergeResult::AlreadyUpToDate
                } else {
                    MergeResult::Merged
                }
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            MergeResult::Failed(stderr)
        }
        Err(e) => MergeResult::Failed(e.to_string()),
    };

    if stashed && pop_stash(repo_dir) {
        aid_info!("[aid] Restored local changes");
    } else if stashed {
        aid_hint!("[aid] Your stashed local changes conflict with the merge. Resolve with: git stash pop");
    }
    merge_result
}

pub(crate) fn check_merge(repo_dir: &str, branch: &str) -> MergeCheckResult {
    let ahead = commits_ahead(repo_dir, branch);
    let stashed = stash_local_changes(repo_dir);
    let output = Command::new("git")
        .args(["-C", repo_dir, "merge", "--no-commit", "--no-ff", branch])
        .output();
    let result = match output {
        Ok(out) if out.status.success() => MergeCheckResult::Ok(ahead),
        Ok(_) => MergeCheckResult::Conflict(conflict_files(repo_dir)),
        Err(err) => MergeCheckResult::Conflict(vec![err.to_string()]),
    };
    abort_merge(repo_dir);
    if stashed && !pop_stash(repo_dir) {
        aid_hint!("[aid] Your stashed local changes conflict with the merge check. Resolve with: git stash pop");
    }
    result
}

pub(crate) enum MergeResult {
    Merged,
    AlreadyUpToDate,
    Failed(String),
}

pub(crate) enum MergeCheckResult {
    Ok(u32),
    Conflict(Vec<String>),
}

fn stash_local_changes(repo_dir: &str) -> bool {
    let dirty = Command::new("git")
        .args(["-C", repo_dir, "status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    if !dirty {
        return false;
    }
    aid_info!("[aid] Stashing local changes before merge...");
    match Command::new("git")
        .args(["-C", repo_dir, "stash", "push", "-m", "aid: auto-stash before merge"])
        .output()
    {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            aid_warn!("[aid] Warning: failed to stash local changes: {}", stderr.lines().next().unwrap_or(""));
            false
        }
        Err(e) => { aid_warn!("[aid] Warning: failed to stash local changes: {e}"); false }
    }
}

fn pop_stash(repo_dir: &str) -> bool {
    match Command::new("git")
        .args(["-C", repo_dir, "stash", "pop"])
        .output()
    {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

fn abort_merge(repo_dir: &str) {
    let _ = Command::new("git").args(["-C", repo_dir, "merge", "--abort"]).output();
}

fn conflict_files(repo_dir: &str) -> Vec<String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "diff", "--name-only", "--diff-filter=U"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let files: Vec<String> = String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            if files.is_empty() { unknown_conflict_files() } else { files }
        }
        _ => unknown_conflict_files(),
    }
}

fn unknown_conflict_files() -> Vec<String> {
    vec!["merge failed without reported conflict files".to_string()]
}

/// Sandbox guard for worktree cleanup paths.
pub fn is_safe_worktree_path(wt_path: &str) -> bool {
    if !Path::new(wt_path).is_absolute() {
        return false;
    }
    let canonical = match Path::new(wt_path).canonicalize() {
        Ok(p) => p,
        Err(_) => return wt_path.starts_with("/tmp/aid-wt-")
            || wt_path.starts_with("/private/tmp/aid-wt-"),
    };
    let s = canonical.to_string_lossy();
    s.starts_with("/tmp/aid-wt-") || s.starts_with("/private/tmp/aid-wt-")
}

pub fn remove_worktree(repo_dir: &str, wt_path: &str) -> Result<()> {
    // SANDBOX: refuse to touch anything outside /tmp/aid-wt-*
    if !is_safe_worktree_path(wt_path) {
        return Err(anyhow!(
            "[aid] SAFETY: refusing to remove '{}' — not an aid worktree path. \
             Only /tmp/aid-wt-* paths are allowed.",
            wt_path
        ));
    }

    let result = Command::new("git")
        .args(["-C", repo_dir, "worktree", "remove", "--force", wt_path])
        .output();
    if matches!(result, Ok(ref out) if out.status.success()) {
        aid_info!("[aid] Cleaned up worktree {wt_path}");
        return Ok(());
    }

    // Fallback: rm -rf, but ONLY after sandbox validation above
    let delete_path = Path::new(wt_path);
    let git_file = delete_path.join(".git");
    if !git_file.is_file() {
        return Err(anyhow!(
            "[aid] SAFETY: refusing fallback removal for '{}' — missing worktree .git file",
            wt_path
        ));
    }
    let canonical = delete_path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize worktree path '{wt_path}' before deletion"))?;
    let canonical_str = canonical.to_string_lossy().to_string();
    if !is_safe_worktree_path(&canonical_str) {
        return Err(anyhow!(
            "[aid] SAFETY: refusing fallback removal for '{}' — canonical path '{}' is outside /tmp/aid-wt-*",
            wt_path,
            canonical_str
        ));
    }

    match fs::remove_dir_all(&canonical) {
        Ok(()) => {
            aid_info!("[aid] Cleaned up worktree {wt_path}");
            let _ = Command::new("git")
                .args(["-C", repo_dir, "worktree", "prune"])
                .output();
            Ok(())
        }
        Err(e) => Err(e).with_context(|| format!("failed to remove worktree '{wt_path}'")),
    }
}
