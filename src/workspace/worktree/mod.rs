// Git worktree management: create, remove, and diff isolated worktrees.
// Used by `aid run --worktree` and `aid batch` for parallel conflict-free dispatch.

use crate::support::sanitize;
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
mod baseline;
mod create;
pub use create::create_worktree;
mod exclude;
mod live_state;
mod lock;
mod path;
mod reconcile;
mod snapshot;
mod staging;
mod state;
mod status;
mod validation;
pub(crate) use baseline::{baseline_contains, extract_baseline_paths};
pub(crate) use live_state::{
    LiveWorktreeState, capture_live_worktree_state, uncommitted_diff_text,
};
pub(crate) use lock::live_lock_holder_with_store;
#[cfg(test)]
pub(crate) use lock::{
    check_worktree_lock, check_worktree_lock_with_store, simulate_stale_recovery_race,
    try_acquire_worktree_lock, write_worktree_lock,
};
pub use lock::{
    clear_worktree_lock, rekey_worktree_lock_to_worker, try_acquire_worktree_lock_with_store,
};
#[cfg(test)]
pub use path::aid_worktree_root;
#[cfg(test)]
pub use path::is_safe_worktree_path;
#[cfg(test)]
pub(crate) use path::remove_worktree;
pub use path::{aid_worktree_path, is_aid_managed_worktree_path};
pub(crate) use snapshot::{
    WorktreeStatusEntry, WorktreeStatusKind, capture_worktree_snapshot,
    capture_worktree_snapshot_with_base,
};
pub(crate) use staging::{AidStageMode, stage_aid_files, stage_aid_files_with_index};
pub(crate) use state::branch_tip_resume_base;
pub use state::{branch_has_commits_ahead_of_main, process_alive_check, worktree_changed_files};
use state::{existing_worktree_path, local_branch_exists, sync_cargo_lock};
pub(crate) use status::{extract_baseline_path, status_line_path};
pub(crate) use validation::ensure_consumed_worktree_path_is_isolated;
pub use validation::validate_git_repo;
use validation::{
    canonical_worktree_path, ensure_current_checkout_is_not_task_target,
    ensure_worktree_path_is_isolated, is_valid_git_worktree,
};

const AID_BRANCH_PREFIXES: &[&str] = &["feat/", "fix/", "docs/", "chore/", "test/", "refactor/"];

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    pub created: bool,
}

fn invalid_worktree_error(path: &Path, branch: &str) -> anyhow::Error {
    anyhow!(
        "Worktree path {} already exists for branch {branch} but is not a shared-ref worktree for this repo; refusing to replace it automatically",
        path.display()
    )
}

fn worktree_create_error(
    path: &Path,
    branch: &str,
    reason: impl std::fmt::Display,
) -> anyhow::Error {
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
    requested_branch: Option<&str>,
    repo_path: Option<&str>,
    wt_path: Option<&str>,
) -> Result<()> {
    let (Some(branch), Some(repo), Some(wt)) = (requested_branch, repo_path, wt_path) else {
        return Ok(());
    };
    ensure_worktree_path_is_isolated(
        Path::new(repo),
        Path::new(wt),
        &format!("--worktree branch '{branch}'"),
    )
}

fn main_checkout_has_branch(repo_dir: &Path, branch: &str) -> Result<bool> {
    let main_dir = path::main_working_tree_dir(repo_dir)?;
    let out = Command::new("git")
        .args([
            "-C",
            &main_dir.to_string_lossy(),
            "rev-parse",
            "--abbrev-ref",
            "HEAD",
        ])
        .output()
        .context("Failed to run git rev-parse --abbrev-ref HEAD")?;
    Ok(out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == branch)
}

fn checked_worktree_info(
    repo_dir: &Path,
    path: PathBuf,
    branch: &str,
    created: bool,
) -> Result<WorktreeInfo> {
    ensure_worktree_path_is_isolated(repo_dir, &path, &format!("--worktree branch '{branch}'"))?;
    Ok(WorktreeInfo {
        path,
        branch: branch.to_string(),
        created,
    })
}

/// Sync repo-backed context files into the worktree when they are missing there.
pub fn sync_context_files_into_worktree(
    repo_dir: &Path,
    wt_path: &Path,
    context_files: &[String],
) -> Vec<String> {
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
        if wt_file.exists() || !repo_file.exists() {
            continue;
        }
        if let Some(parent) = wt_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(&repo_file, &wt_file).is_ok() {
            synced.push(rel_path.display().to_string());
        }
    }
    synced
}

fn is_aid_managed_branch(branch: &str) -> bool {
    AID_BRANCH_PREFIXES
        .iter()
        .any(|prefix| branch.starts_with(prefix))
}

pub(crate) mod deps;
pub(crate) mod gc;
pub(crate) mod layout;
#[cfg(test)]
mod lock_reentry_tests;
#[cfg(test)]
mod lock_tests;
#[cfg(test)]
mod main_checkout_tests;
#[cfg(test)]
mod path_tests;
#[cfg(test)]
mod resume_tests;
#[cfg(test)]
mod stale_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod validation_tests;
