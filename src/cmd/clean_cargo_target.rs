// Cargo target cleanup for `aid clean --worktrees`.
// Exports orphaned branch target cleanup while preserving `_base` and live worktrees.
// Deps: crate::agent env helpers, crate::worktree path guards, git CLI, std fs/path.

use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BASE_TARGET_DIR_NAME: &str = "_base";

pub(crate) fn clean_orphaned_branch_targets(dry_run: bool) -> Result<()> {
    let Some(root) = crate::agent::env::branch_target_root() else {
        return Ok(());
    };
    let live_names = live_branch_target_names()?;
    let mut removed = 0usize;
    for target in branch_target_dirs(&root)? {
        let Some(name) = target.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if is_reserved_target_dir_name(name) || live_names.contains(name) {
            continue;
        }
        if dry_run {
            println!("[dry-run] Would remove orphaned Cargo target dir {}", target.display());
        } else {
            fs::remove_dir_all(&target)?;
            println!("Removed orphaned Cargo target dir {}", target.display());
        }
        removed += 1;
    }
    println!(
        "{} {removed} orphaned Cargo target dirs",
        if dry_run { "[dry-run] Would remove" } else { "Removed" }
    );
    Ok(())
}

fn branch_target_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn live_branch_target_names() -> Result<HashSet<String>> {
    let mut names = HashSet::new();
    collect_live_names_under(&crate::worktree::aid_worktree_root(), &mut names)?;
    collect_live_names_under(Path::new("/tmp"), &mut names)?;
    Ok(names)
}

fn collect_live_names_under(root: &Path, names: &mut HashSet<String>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        if crate::worktree::is_aid_managed_worktree_path(&path) {
            insert_current_branch_name(&path, names);
        }
        if path.starts_with(crate::worktree::aid_worktree_root()) {
            collect_live_names_under(&path, names)?;
        }
    }
    Ok(())
}

fn insert_current_branch_name(path: &Path, names: &mut HashSet<String>) {
    let Some(branch) = current_branch(path) else {
        return;
    };
    names.insert(crate::agent::env::branch_target_name(&branch));
}

fn current_branch(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty() && branch != "HEAD").then_some(branch)
}

fn is_reserved_target_dir_name(name: &str) -> bool {
    name == BASE_TARGET_DIR_NAME
        || name.starts_with('.')
        || matches!(name, "debug" | "release" | "tmp" | "doc" | "package")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_subprocess;
    use std::ffi::{OsStr, OsString};

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    fn git(repo_dir: &Path, args: &[&str]) {
        assert!(Command::new("git")
            .args(["-C", &repo_dir.to_string_lossy()])
            .args(args)
            .status()
            .unwrap()
            .success());
    }

    fn init_repo(repo_dir: &Path) {
        git(repo_dir, &["init", "-b", "main"]);
        git(repo_dir, &["config", "user.email", "test@example.com"]);
        git(repo_dir, &["config", "user.name", "Test User"]);
        fs::write(repo_dir.join("file.txt"), "hello\n").unwrap();
        git(repo_dir, &["add", "file.txt"]);
        git(repo_dir, &["commit", "-m", "init"]);
    }

    #[test]
    fn clean_branch_targets_keeps_base_and_live_worktree_target() {
        let _permit = test_subprocess::acquire();
        let aid_home = tempfile::tempdir().unwrap();
        let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        let root = aid_home.path().join("cargo-target");
        let _target_guard = EnvVarGuard::set("CARGO_TARGET_DIR", &root);
        let live_branch = "feat/live-clean-target";
        let stale_target = root.join("feat-stale-clean-target");
        let live_target = root.join("feat-live-clean-target");
        let wt_path = Path::new("/tmp").join(format!("aid-wt-live-clean-target-{}", std::process::id()));
        let _ = fs::remove_dir_all(&wt_path);
        fs::create_dir_all(root.join(BASE_TARGET_DIR_NAME)).unwrap();
        fs::create_dir_all(&stale_target).unwrap();
        fs::create_dir_all(&live_target).unwrap();
        git(repo.path(), &["worktree", "add", &wt_path.to_string_lossy(), "-b", live_branch]);

        clean_orphaned_branch_targets(false).unwrap();

        assert!(root.join(BASE_TARGET_DIR_NAME).exists());
        assert!(live_target.exists());
        assert!(!stale_target.exists());
        git(repo.path(), &["worktree", "remove", "--force", &wt_path.to_string_lossy()]);
    }
}
