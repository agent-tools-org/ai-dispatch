// Aid-managed worktree path helpers.
// Exports root/path generation, sandbox path classification, and sandboxed worktree removal.
// Deps: std::env, std::path, and test-only thread-local overrides.

use anyhow::{Context, Result, anyhow};
#[cfg(test)]
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(test)]
thread_local! {
    static WORKTREE_HOME_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Root directory for aid-managed worktrees.
pub fn aid_worktree_root() -> PathBuf {
    #[cfg(test)]
    {
        if let Some(home) = WORKTREE_HOME_OVERRIDE.with(|cell| cell.borrow().clone()) {
            return home.join(".aid").join("worktrees");
        }
        std::env::temp_dir()
            .join(format!("aid-test-home-{}", std::process::id()))
            .join(".aid")
            .join("worktrees")
    }

    #[cfg(not(test))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .map(|home| home.join(".aid").join("worktrees"))
            .unwrap_or_else(|| PathBuf::from("/tmp/aid-wt-fallback"))
    }
}

#[cfg(test)]
pub(super) struct WorktreeHomeGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl WorktreeHomeGuard {
    pub(super) fn set(path: &Path) -> Self {
        let previous = WORKTREE_HOME_OVERRIDE.with(|cell| cell.borrow().clone());
        WORKTREE_HOME_OVERRIDE.with(|cell| *cell.borrow_mut() = Some(path.to_path_buf()));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for WorktreeHomeGuard {
    fn drop(&mut self) {
        WORKTREE_HOME_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous.take());
    }
}

/// Path for an aid-managed worktree. Callers must create the parent directory before use.
pub fn aid_worktree_path(repo_dir: &Path, branch: &str) -> PathBuf {
    aid_worktree_root()
        .join(project_id(&main_repo_dir(repo_dir)))
        .join(branch)
}

pub(super) fn main_repo_dir(repo_dir: &Path) -> PathBuf {
    main_working_tree_dir(repo_dir).unwrap_or_else(|_| legacy_main_repo_dir(repo_dir))
}

pub(super) fn main_working_tree_dir(repo_dir: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "list", "--porcelain"])
        .output()
        .with_context(|| format!("cannot determine main checkout for {}", repo_dir.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "cannot determine main checkout for {}: {}",
            repo_dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(path) = line.strip_prefix("worktree ") else {
            continue;
        };
        let path = path.trim();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    Err(anyhow!(
        "cannot determine main checkout for {}: git worktree list returned no worktree entries",
        repo_dir.display()
    ))
}

fn legacy_main_repo_dir(repo_dir: &Path) -> PathBuf {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "rev-parse",
            "--git-common-dir",
        ])
        .output();
    if let Ok(output) = output {
        if output.status.success() {
            let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let common = if Path::new(&raw).is_absolute() {
                PathBuf::from(raw)
            } else {
                repo_dir.join(raw)
            };
            if let Some(parent) = common.parent() {
                return parent.to_path_buf();
            }
        }
    }
    repo_dir.to_path_buf()
}

fn project_id(repo_dir: &Path) -> String {
    let canonical = repo_dir.canonicalize().ok();
    let basename = canonical
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "default".to_string());
    let hash = canonical
        .as_ref()
        .map(|path| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            path.to_string_lossy().hash(&mut hasher);
            format!("{:x}", hasher.finish())
        })
        .unwrap_or_else(|| "0".to_string());
    let hash_short: String = hash.chars().take(8).collect();
    format!("{basename}-{hash_short}")
}

/// True when a path is under aid's current worktree root or legacy /tmp worktree names.
pub fn is_aid_managed_worktree_path(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let normalized = path
        .canonicalize()
        .unwrap_or_else(|_| logical_normalize(path));
    let root = aid_worktree_root();
    let root_canonical = root
        .canonicalize()
        .unwrap_or_else(|_| logical_normalize(&root));
    if normalized.starts_with(&root_canonical) {
        return true;
    }
    let path = normalized.to_string_lossy();
    path.starts_with("/tmp/aid-wt-") || path.starts_with("/private/tmp/aid-wt-")
}

/// Sandbox guard for worktree cleanup paths.
pub fn is_safe_worktree_path(wt_path: &str) -> bool {
    if !Path::new(wt_path).is_absolute() {
        return false;
    }
    let canonical = match Path::new(wt_path).canonicalize() {
        Ok(p) => p,
        Err(_) => return is_aid_managed_worktree_path(Path::new(wt_path)),
    };
    is_aid_managed_worktree_path(&canonical)
}

#[cfg(test)]
pub(crate) fn remove_worktree(repo_dir: &str, wt_path: &str) -> Result<()> {
    anyhow::ensure!(is_safe_worktree_path(wt_path), "unsafe worktree path");
    let status = Command::new("git")
        .args(["-C", repo_dir, "worktree", "remove", "--force", wt_path])
        .status()?;
    anyhow::ensure!(status.success(), "git worktree remove failed");
    Ok(())
}

fn logical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
