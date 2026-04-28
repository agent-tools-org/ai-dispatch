// Aid-managed worktree path helpers.
// Exports root/path generation and sandbox path classification.
// Deps: std::env, std::path, and test-only thread-local overrides.

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

fn main_repo_dir(repo_dir: &Path) -> PathBuf {
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
