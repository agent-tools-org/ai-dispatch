// Worktree creation and reuse phases.
// Uses parent validation, branch state and custody checks.

use super::*;

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
    exclude::ensure_aid_paths_excluded(repo_dir);
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
        return reuse_worktree(repo_dir, wt_path, branch, base_branch);
    }
    add_worktree(repo_dir, wt_path, branch, base_branch)
}

fn add_worktree(
    repo_dir: &Path,
    wt_path: PathBuf,
    branch: &str,
    base_branch: Option<&str>,
) -> Result<WorktreeInfo> {
    // Try new branch first
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "add", &wt_path.to_string_lossy(), "-b", branch])
        .args(base_branch)
        .output()
        .map_err(|err| {
            worktree_create_error(
                &wt_path,
                branch,
                format!("failed to run git worktree add: {err}"),
            )
        })?;

    if out.status.success() {
        ensure_worktree_path_is_isolated(
            repo_dir,
            &wt_path,
            &format!("--worktree branch '{branch}'"),
        )?;
        sync_cargo_lock(repo_dir, &wt_path);
        return checked_worktree_info(repo_dir, wt_path, branch, true);
    }

    if let Some(info) = reuse_registered_worktree(repo_dir, branch, base_branch)? {
        return Ok(info);
    }

    add_existing_branch_worktree(repo_dir, wt_path, branch, base_branch)
}

fn reuse_worktree(
    repo_dir: &Path,
    wt_path: PathBuf,
    branch: &str,
    base_branch: Option<&str>,
) -> Result<WorktreeInfo> {
    let expected_path = validate_reused_worktree(repo_dir, &wt_path, branch)?;
    if is_valid_git_worktree(repo_dir, &wt_path)? {
        if let Some(existing_path) = existing_worktree_path(repo_dir, branch)? {
            if existing_path.exists() && canonical_worktree_path(&existing_path) != expected_path {
                ensure_worktree_path_is_isolated(
                    repo_dir,
                    &existing_path,
                    &format!("--worktree branch '{branch}'"),
                )?;
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
        ensure_worktree_path_is_isolated(
            repo_dir,
            &wt_path,
            &format!("--worktree branch '{branch}'"),
        )?;
        ensure_live_worktree_unlocked(&wt_path)?;
        reconcile::maybe_refresh_existing_worktree(repo_dir, &wt_path, branch, base_branch)?;
        sync_cargo_lock(repo_dir, &wt_path);
        return checked_worktree_info(repo_dir, wt_path, branch, false);
    }

    return Err(invalid_worktree_error(&wt_path, branch));
}

fn add_existing_branch_worktree(
    repo_dir: &Path,
    wt_path: PathBuf,
    branch: &str,
    base_branch: Option<&str>,
) -> Result<WorktreeInfo> {
    // Fallback after `git worktree add -b` fails, usually because the branch already exists.
    let branch_exists = local_branch_exists(repo_dir, branch)?;
    if branch_exists && main_checkout_has_branch(repo_dir, branch)? {
        return Err(main_worktree_branch_error(branch));
    }
    let is_branch_tip_resume =
        branch_exists && (base_branch.is_none() || base_branch == Some(branch));
    if !is_branch_tip_resume {
        reset_existing_branch(repo_dir, branch, base_branch, branch_exists)?;
    }
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "add", &wt_path.to_string_lossy(), branch])
        .output()
        .map_err(|err| {
            worktree_create_error(
                &wt_path,
                branch,
                format!("failed to run git worktree add: {err}"),
            )
        })?;
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

fn validate_reused_worktree(repo_dir: &Path, wt_path: &Path, branch: &str) -> Result<PathBuf> {
    // Reject symlinks to prevent symlink-following attacks on /tmp paths
    if wt_path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        anyhow::bail!(
            "Worktree path {} is a symlink — refusing to use for safety",
            wt_path.display()
        );
    }
    let expected_path = canonical_worktree_path(wt_path);
    if existing_worktree_path(repo_dir, branch)?
        .is_some_and(|path| canonical_worktree_path(&path) != expected_path)
    {
        anyhow::bail!(
            "Branch '{branch}' has conflicting worktree metadata; automatic pruning is forbidden \
            because the registration may own unaccepted artifacts"
        );
    }
    Ok(expected_path)
}

fn reset_existing_branch(
    repo_dir: &Path,
    branch: &str,
    base_branch: Option<&str>,
    branch_exists: bool,
) -> Result<()> {
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
    Ok(())
}

fn reuse_registered_worktree(
    repo_dir: &Path,
    branch: &str,
    base_branch: Option<&str>,
) -> Result<Option<WorktreeInfo>> {
    if let Some(existing_path) = existing_worktree_path(repo_dir, branch)? {
        if existing_path.exists() {
            ensure_worktree_path_is_isolated(
                repo_dir,
                &existing_path,
                &format!("--worktree branch '{branch}'"),
            )?;
            ensure_live_worktree_unlocked(&existing_path)?;
            reconcile::maybe_refresh_existing_worktree(
                repo_dir,
                &existing_path,
                branch,
                base_branch,
            )?;
            sync_cargo_lock(repo_dir, &existing_path);
            return checked_worktree_info(repo_dir, existing_path, branch, false).map(Some);
        }

        anyhow::bail!(
            "Branch '{branch}' has a missing worktree registration; automatic pruning is forbidden \
            because its private object store may contain unaccepted artifacts"
        );
    }

    Ok(None)
}
