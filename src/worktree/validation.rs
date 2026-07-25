// Linked-worktree validation helpers for aid-managed /tmp worktrees.
// Exports: path canonicalization, main-checkout detection, and git worktree validation.
// Deps: anyhow, std::path, std::process::Command.

use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::process::Command;
use super::path::main_working_tree_dir;

pub(super) fn canonical_worktree_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn is_main_working_tree_path(repo_dir: &Path, wt_path: &Path) -> bool {
    main_working_tree_dir(repo_dir)
        .ok()
        .is_some_and(|main| canonical_worktree_path(wt_path) == canonical_worktree_path(&main))
}

pub(crate) fn ensure_worktree_path_is_isolated(repo_dir: &Path, wt_path: &Path, context: &str) -> Result<()> {
    let wt = canonical_existing_path(wt_path, "worktree path")?;
    let repo = canonical_existing_path(repo_dir, "repo path")?;
    if wt == repo {
        return Err(anyhow!(
            "Refusing to use {context} '{}' because it is the caller checkout. Task worktrees must be isolated from the checkout that dispatched them.",
            wt_path.display()
        ));
    }
    let main = main_working_tree_dir(repo_dir)
        .with_context(|| format!("cannot determine main checkout for {}", repo_dir.display()))?;
    let main = canonical_existing_path(&main, "main checkout")?;
    if wt == main {
        return Err(anyhow!(
            "Refusing to use {context} '{}' because it is the main working tree. Task worktrees must be isolated linked worktrees.",
            wt_path.display()
        ));
    }
    Ok(())
}

pub(crate) fn ensure_consumed_worktree_path_is_isolated(
    repo_path: Option<&str>,
    wt_path: &str,
    context: &str,
) -> Result<()> {
    let wt = Path::new(wt_path);
    if let Some(repo) = repo_path {
        return ensure_worktree_path_is_isolated(Path::new(repo), wt, context);
    }
    let main = main_working_tree_dir(wt)
        .with_context(|| format!("cannot determine main checkout for {}", wt.display()))?;
    let wt = canonical_existing_path(wt, "worktree path")?;
    let main = canonical_existing_path(&main, "main checkout")?;
    if wt == main {
        return Err(anyhow!(
            "Refusing to use {context} because it is the main working tree. Task worktrees must be isolated linked worktrees."
        ));
    }
    Ok(())
}

pub(super) fn ensure_current_checkout_is_not_task_target(repo_dir: &Path, branch: &str) -> Result<()> {
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .context("Failed to run git rev-parse --abbrev-ref HEAD")?;
    if !out.status.success() || String::from_utf8_lossy(&out.stdout).trim() != branch {
        return Ok(());
    }
    let repo = canonical_existing_path(repo_dir, "repo path")?;
    let main = canonical_existing_path(&main_working_tree_dir(repo_dir)?, "main checkout")?;
    if repo == main {
        return Err(anyhow!("Refusing to use the main working tree as an aid task worktree for branch '{branch}'. The branch is checked out in the main working tree; switch the main checkout to another branch or pick a different --worktree name."));
    }
    Err(anyhow!("Refusing to use the caller checkout as an aid task worktree for branch '{branch}'. The branch is checked out in the checkout that dispatched this task; switch that checkout to another branch or pick a different --worktree name."))
}

fn canonical_existing_path(path: &Path, label: &str) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("failed to canonicalize {label} {}", path.display()))
}

pub(super) fn is_valid_git_worktree(repo_dir: &Path, wt_path: &Path) -> Result<bool> {
    if !git_rev_parse_succeeds(wt_path, "--git-dir")? {
        return Ok(false);
    }

    let Some(repo_common_dir) = git_common_dir(repo_dir)? else {
        return Ok(false);
    };
    let Some(wt_common_dir) = git_common_dir(wt_path)? else {
        return Ok(false);
    };
    if repo_common_dir != wt_common_dir {
        return Ok(false);
    }

    is_registered_worktree(repo_dir, wt_path)
}

fn git_rev_parse_succeeds(path: &Path, arg: &str) -> Result<bool> {
    let status = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", arg])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to run git rev-parse {arg}"))?;
    Ok(status.success())
}

fn git_common_dir(path: &Path) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--git-common-dir"])
        .output()
        .context("Failed to run git rev-parse --git-common-dir")?;
    if !output.status.success() {
        return Ok(None);
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Ok(None);
    }

    let resolved = resolve_git_path(path, Path::new(raw.as_str()));
    Ok(resolved.canonicalize().ok())
}

fn resolve_git_path(base_dir: &Path, git_path: &Path) -> PathBuf {
    if git_path.is_absolute() {
        git_path.to_path_buf()
    } else {
        base_dir.join(git_path)
    }
}

fn is_registered_worktree(repo_dir: &Path, wt_path: &Path) -> Result<bool> {
    let expected = wt_path.canonicalize().ok();
    let Some(expected) = expected else {
        return Ok(false);
    };

    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "worktree", "list", "--porcelain"])
        .output()
        .context("Failed to run git worktree list --porcelain")?;
    if !output.status.success() {
        return Ok(false);
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(path) = line.strip_prefix("worktree ") else {
            continue;
        };
        let listed = Path::new(path.trim()).canonicalize().ok();
        if listed.as_ref().is_some_and(|listed| listed == &expected) {
            return Ok(true);
        }
    }

    Ok(false)
}
