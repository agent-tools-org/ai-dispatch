// Git worktree management: create, remove, and diff isolated worktrees.
// Used by `aid run --worktree` and `aid batch` for parallel conflict-free dispatch.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use crate::sanitize;
#[path = "worktree/reconcile.rs"]
mod reconcile;
#[path = "worktree/snapshot.rs"]
mod snapshot;
#[path = "worktree/baseline.rs"]
mod baseline;
#[path = "worktree/live_state.rs"]
mod live_state;
#[path = "worktree/lock.rs"]
mod lock;
#[path = "worktree/state.rs"]
mod state;
#[path = "worktree/validation.rs"]
mod validation;
#[path = "worktree/path.rs"]
mod path;
pub(crate) use snapshot::{WorktreeStatusEntry, WorktreeStatusKind, capture_worktree_snapshot, capture_worktree_snapshot_with_base};
pub(crate) use live_state::{LiveWorktreeState, capture_live_worktree_state, uncommitted_diff_text};
pub(crate) use baseline::{baseline_contains, extract_baseline_path, extract_baseline_paths};
pub use path::{aid_worktree_path, aid_worktree_root, is_aid_managed_worktree_path};
#[cfg(test)]
pub use path::is_safe_worktree_path;
#[cfg(test)]
pub(crate) use path::remove_worktree;
pub(crate) use state::branch_tip_resume_base;
pub use state::{branch_has_commits_ahead_of_main, process_alive_check, worktree_changed_files};
pub use lock::{clear_worktree_lock, rekey_worktree_lock_to_worker, try_acquire_worktree_lock_with_store};
pub(crate) use lock::live_lock_holder_with_store;
#[cfg(test)]
pub(crate) use lock::{
    check_worktree_lock, check_worktree_lock_with_store, simulate_stale_recovery_race,
    try_acquire_worktree_lock, write_worktree_lock,
};
use state::{existing_worktree_path, local_branch_exists, sync_cargo_lock};
use validation::{canonical_worktree_path, ensure_current_checkout_is_not_task_target, ensure_worktree_path_is_isolated, is_valid_git_worktree};
pub(crate) use validation::ensure_consumed_worktree_path_is_isolated;

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

fn invalid_worktree_error(path: &Path, branch: &str) -> anyhow::Error {
    anyhow!(
        "Worktree path {} already exists for branch {branch} but is not a shared-ref worktree for this repo; refusing to replace it automatically",
        path.display()
    )
}

fn worktree_create_error(path: &Path, branch: &str, reason: impl std::fmt::Display) -> anyhow::Error {
    anyhow!(
        "Failed to create worktree at {} for branch {}: {}. Destructive cleanup requires principal acceptance and custody GC",
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

fn ensure_live_worktree_unlocked(path: &Path) -> Result<()> {
    if let Some(holder) = lock::live_lock_holder(path) {
        anyhow::bail!("Worktree {} is locked by task {holder} — concurrent access prevented. Use separate worktree names for parallel tasks.", path.display());
    }
    Ok(())
}

fn main_worktree_branch_error(branch: &str) -> anyhow::Error {
    anyhow!("Refusing to use the main working tree as an aid task worktree for branch '{branch}'. The branch is checked out in the main working tree; switch the main checkout to another branch or pick a different --worktree name.")
}

pub(crate) fn ensure_requested_worktree_is_isolated(
    requested_branch: Option<&str>, repo_path: Option<&str>, wt_path: Option<&str>,
) -> Result<()> {
    let (Some(branch), Some(repo), Some(wt)) = (requested_branch, repo_path, wt_path) else { return Ok(()) };
    ensure_worktree_path_is_isolated(Path::new(repo), Path::new(wt), &format!("--worktree branch '{branch}'"))
}

fn main_checkout_has_branch(repo_dir: &Path, branch: &str) -> Result<bool> {
    let main_dir = path::main_working_tree_dir(repo_dir)?;
    let out = Command::new("git")
        .args(["-C", &main_dir.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to run git rev-parse --abbrev-ref HEAD")?;
    Ok(out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == branch)
}

fn checked_worktree_info(repo_dir: &Path, path: PathBuf, branch: &str, created: bool) -> Result<WorktreeInfo> {
    ensure_worktree_path_is_isolated(repo_dir, &path, &format!("--worktree branch '{branch}'"))?;
    Ok(WorktreeInfo { path, branch: branch.to_string(), created })
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

pub fn create_worktree(repo_dir: &Path, branch: &str, base_branch: Option<&str>) -> Result<WorktreeInfo> {
    sanitize::validate_branch_name(branch)?;
    if let Some(base_branch) = base_branch {
        sanitize::validate_branch_name(base_branch)?;
    }
    validate_git_repo(repo_dir)?;
    ensure_current_checkout_is_not_task_target(repo_dir, branch)?;
    let wt_path = aid_worktree_path(repo_dir, branch);
    if let Some(parent) = wt_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create aid worktree parent directory {}",
                parent.display()
            )
        })?;
    }

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
            anyhow::bail!(
                "Branch '{branch}' has conflicting worktree metadata; automatic pruning is forbidden because the registration may own unaccepted artifacts"
            );
        }
        if is_valid_git_worktree(repo_dir, &wt_path)? {
            if let Some(existing_path) = existing_worktree_path(repo_dir, branch)? {
                if existing_path.exists()
                    && canonical_worktree_path(&existing_path) != expected_path
                {
                    ensure_worktree_path_is_isolated(repo_dir, &existing_path, &format!("--worktree branch '{branch}'"))?;
                    ensure_live_worktree_unlocked(&existing_path)?;
                    reconcile::maybe_refresh_existing_worktree(
                        repo_dir,
                        &existing_path,
                        branch,
                        base_branch,
                    )?;
                    sync_cargo_lock(repo_dir, &existing_path);
                    return checked_worktree_info(repo_dir, existing_path, branch, false);
                }
            }
            ensure_worktree_path_is_isolated(repo_dir, &wt_path, &format!("--worktree branch '{branch}'"))?;
            ensure_live_worktree_unlocked(&wt_path)?;
            reconcile::maybe_refresh_existing_worktree(repo_dir, &wt_path, branch, base_branch)?;
            sync_cargo_lock(repo_dir, &wt_path);
            return checked_worktree_info(repo_dir, wt_path, branch, false);
        }

        return Err(invalid_worktree_error(&wt_path, branch));
    }

    // Try new branch first
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "add", &wt_path.to_string_lossy(), "-b", branch])
        .args(base_branch)
        .output()
        .map_err(|err| worktree_create_error(&wt_path, branch, format!("failed to run git worktree add: {err}")))?;

    if out.status.success() {
        ensure_worktree_path_is_isolated(repo_dir, &wt_path, &format!("--worktree branch '{branch}'"))?;
        sync_cargo_lock(repo_dir, &wt_path);
        return checked_worktree_info(repo_dir, wt_path, branch, true);
    }

    if let Some(existing_path) = existing_worktree_path(repo_dir, branch)? {
        if existing_path.exists() {
            ensure_worktree_path_is_isolated(repo_dir, &existing_path, &format!("--worktree branch '{branch}'"))?;
            ensure_live_worktree_unlocked(&existing_path)?;
            reconcile::maybe_refresh_existing_worktree(repo_dir, &existing_path, branch, base_branch)?;
            sync_cargo_lock(repo_dir, &existing_path);
            return checked_worktree_info(repo_dir, existing_path, branch, false);
        }

        anyhow::bail!(
            "Branch '{branch}' has a missing worktree registration; automatic pruning is forbidden because its private object store may contain unaccepted artifacts"
        );
    }

    // Fallback after `git worktree add -b` fails, usually because the branch already exists.
    let branch_exists = local_branch_exists(repo_dir, branch)?;
    if branch_exists && main_checkout_has_branch(repo_dir, branch)? { return Err(main_worktree_branch_error(branch)); }
    let is_branch_tip_resume = branch_exists && (base_branch.is_none() || base_branch == Some(branch));
    if !is_branch_tip_resume {
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
        let reset_base = if branch_exists {
            reconcile::ensure_branch_force_reset_is_safe(repo_dir, branch, base_branch)?
        } else {
            base_branch.unwrap_or("HEAD").to_string()
        };
        let _ = Command::new("git")
            .args([
                "-C",
                &repo_dir.to_string_lossy(),
                "branch",
                "-f",
                branch,
                &reset_base,
            ])
            .output();
    }
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
    ensure_worktree_path_is_isolated(repo_dir, &wt_path, &format!("--worktree branch '{branch}'"))?;
    sync_cargo_lock(repo_dir, &wt_path);
    checked_worktree_info(repo_dir, wt_path, branch, true)
}

#[cfg(test)] mod tests;
#[cfg(test)] #[path = "worktree/main_checkout_tests.rs"] mod main_checkout_tests;
#[cfg(test)] #[path = "worktree/path_tests.rs"] mod path_tests;
#[cfg(test)] #[path = "worktree/resume_tests.rs"] mod resume_tests;
#[cfg(test)] #[path = "worktree/stale_tests.rs"] mod stale_tests;
#[cfg(test)] #[path = "worktree/validation_tests.rs"] mod validation_tests;
#[cfg(test)] #[path = "worktree/lock_tests.rs"] mod lock_tests;
#[cfg(test)] #[path = "worktree/lock_reentry_tests.rs"] mod lock_reentry_tests;
