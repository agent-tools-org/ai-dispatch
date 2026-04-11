// Git worktree management: create, remove, and diff isolated worktrees.
// Used by `aid run --worktree` and `aid batch` for parallel conflict-free dispatch.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::sanitize;
#[path = "worktree/reconcile.rs"]
mod reconcile;
#[path = "worktree/state.rs"]
mod state;
#[path = "worktree/validation.rs"]
mod validation;
pub use state::{
    branch_has_commits_ahead_of_main, check_worktree_lock, clear_worktree_lock,
    process_alive_check, worktree_changed_files, write_worktree_lock,
};
use state::{existing_worktree_path, local_branch_exists, prune_worktrees, sync_cargo_lock};
use validation::{canonical_worktree_path, is_valid_git_worktree};

const AID_BRANCH_PREFIXES: &[&str] = &["feat/", "fix/", "docs/", "chore/", "test/", "refactor/"];

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    pub created: bool,
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

fn invalid_worktree_warning(path: &Path) {
    aid_warn!(
        "[aid] Warning: Existing path {} is not a shared-ref worktree for this repo; removing it and re-creating a linked worktree",
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

fn is_aid_managed_branch(branch: &str) -> bool {
    AID_BRANCH_PREFIXES
        .iter()
        .any(|prefix| branch.starts_with(prefix))
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
        if is_valid_git_worktree(repo_dir, &wt_path)? {
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
                        created: false,
                    });
                }
            }
            reconcile::maybe_refresh_existing_worktree(repo_dir, &wt_path, branch, base_branch)?;
            sync_cargo_lock(repo_dir, &wt_path);
            return Ok(WorktreeInfo {
                path: wt_path,
                branch: branch.to_string(),
                created: false,
            });
        }

        invalid_worktree_warning(&wt_path);
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
            created: true,
        });
    }

    if let Some(existing_path) = existing_worktree_path(repo_dir, branch)? {
        if existing_path.exists() {
            reconcile::maybe_refresh_existing_worktree(repo_dir, &existing_path, branch, base_branch)?;
            sync_cargo_lock(repo_dir, &existing_path);
            return Ok(WorktreeInfo {
                path: existing_path,
                branch: branch.to_string(),
                created: false,
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
        created: true,
    })
}

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "worktree/stale_tests.rs"]
mod stale_tests;
#[cfg(test)]
#[path = "worktree/validation_tests.rs"]
mod validation_tests;
