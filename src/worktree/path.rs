// Aid-managed worktree path helpers.
// Exports root/path generation and sandbox path classification.
// Deps: std::env, std::path, and test-only thread-local overrides.

#[cfg(test)]
use std::cell::RefCell;
use std::path::{Path, PathBuf};

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
    let project = repo_dir
        .canonicalize()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_owned()))
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "default".to_string());
    aid_worktree_root().join(project).join(branch)
}

/// True when a path is under aid's current worktree root or legacy /tmp worktree names.
pub fn is_aid_managed_worktree_path(path: &Path) -> bool {
    let root = aid_worktree_root();
    if path.starts_with(&root)
        || root
            .canonicalize()
            .is_ok_and(|canonical| path.starts_with(canonical))
    {
        return true;
    }
    let path = path.to_string_lossy();
    path.starts_with("/tmp/aid-wt-") || path.starts_with("/private/tmp/aid-wt-")
}
