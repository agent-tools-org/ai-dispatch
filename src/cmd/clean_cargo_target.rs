// Cargo target cleanup for `aid clean --worktrees`.
// Exports orphaned branch target cleanup while preserving `_base` and live worktrees.
// Deps: crate::agent env helpers, crate::worktree path guards, git CLI, std fs/path.

use anyhow::Result;
use crate::store::Store;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BASE_TARGET_DIR_NAME: &str = "_base";

pub(crate) fn clean_orphaned_branch_targets(dry_run: bool, fallback_root: Option<&Path>) -> Result<u64> {
    let (live_names, mut live_keys) = live_branch_target_info()?;
    let mut removed_cargo = 0usize;
    let mut cargo_bytes = 0u64;

    if let Some(root) = crate::agent::env::branch_target_root() {
        for target in branch_target_dirs(&root)? {
            let Some(name) = target.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if is_reserved_target_dir_name(name) || live_names.contains(name) {
                continue;
            }
            let size = crate::cmd::clean::get_dir_size(&target);
            if dry_run {
                println!("[dry-run] Would remove orphaned Cargo target dir {} ({})", target.display(), crate::cmd::clean::format_bytes(size));
                cargo_bytes += size;
            } else {
                if fs::remove_dir_all(&target).is_ok() {
                    println!("Removed orphaned Cargo target dir {} ({})", target.display(), crate::cmd::clean::format_bytes(size));
                    cargo_bytes += size;
                }
            }
            removed_cargo += 1;
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        live_keys.insert(crate::cmd::build::build_fallback::cwd_key(&cwd));
    }

    let temp_root = fallback_root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| crate::cmd::build::build_fallback::fallback_target_root());
    let mut removed_fallback = 0usize;
    let mut fallback_bytes = 0u64;
    
    let dirs = branch_target_dirs(&temp_root)?;
    for target in dirs {
        let Some(name) = target.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if live_keys.contains(name) {
            continue;
        }
        let size = crate::cmd::clean::get_dir_size(&target);
        if dry_run {
            println!("[dry-run] Would remove orphaned fallback target dir {} ({})", target.display(), crate::cmd::clean::format_bytes(size));
            fallback_bytes += size;
        } else {
            if fs::remove_dir_all(&target).is_ok() {
                println!("Removed orphaned fallback target dir {} ({})", target.display(), crate::cmd::clean::format_bytes(size));
                fallback_bytes += size;
            }
        }
        removed_fallback += 1;
    }

    if removed_cargo > 0 || dry_run {
        println!(
            "{} {removed_cargo} orphaned Cargo target dirs ({})",
            if dry_run { "[dry-run] Would remove" } else { "Removed" },
            crate::cmd::clean::format_bytes(cargo_bytes)
        );
    }
    if removed_fallback > 0 || dry_run {
        println!(
            "{} {removed_fallback} orphaned fallback target dirs ({})",
            if dry_run { "[dry-run] Would remove" } else { "Removed" },
            crate::cmd::clean::format_bytes(fallback_bytes)
        );
    }
    Ok(cargo_bytes + fallback_bytes)
}

pub(crate) fn measure_orphaned_space(store: &Store) -> Result<(u64, u64, u64)> {
    let (live_names, mut live_keys) = live_branch_target_info()?;
    let mut cargo_bytes = 0u64;

    if let Some(root) = crate::agent::env::branch_target_root() {
        let dirs = branch_target_dirs(&root)?;
        for target in dirs {
            let Some(name) = target.file_name().and_then(|name| name.to_str()) else { continue; };
            if !is_reserved_target_dir_name(name) && !live_names.contains(name) {
                cargo_bytes += crate::cmd::clean::get_dir_size(&target);
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        live_keys.insert(crate::cmd::build::build_fallback::cwd_key(&cwd));
    }

    let mut fallback_bytes = 0u64;
    let temp_root = crate::cmd::build::build_fallback::fallback_target_root();
    let dirs = branch_target_dirs(&temp_root)?;
    for target in dirs {
        let Some(name) = target.file_name().and_then(|name| name.to_str()) else { continue; };
        if !live_keys.contains(name) {
            fallback_bytes += crate::cmd::clean::get_dir_size(&target);
        }
    }

    let mut home_bytes = 0u64;
    for id in orphaned_task_ids(store)? {
        let home_dir = crate::paths::task_dir(&id).join("home");
        if home_dir.exists() {
            home_bytes += crate::cmd::clean::get_dir_size(&home_dir);
        }
    }
    
    Ok((cargo_bytes, fallback_bytes, home_bytes))
}

pub(crate) fn orphaned_task_ids(store: &Store) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let conn = store.db();
    let mut stmt = conn.prepare("SELECT id, worktree_path FROM tasks WHERE status NOT IN ('pending', 'running', 'awaiting_input')")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    for row in rows.flatten() {
        let (id, wt_path) = row;
        let is_live = wt_path.map(|p| Path::new(&p).exists()).unwrap_or(false);
        if !is_live {
            ids.push(id);
        }
    }
    Ok(ids)
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

fn live_branch_target_info() -> Result<(HashSet<String>, HashSet<String>)> {
    let mut names = HashSet::new();
    let mut keys = HashSet::new();
    collect_live_info_under(&crate::worktree::aid_worktree_root(), &mut names, &mut keys)?;
    collect_live_info_under(Path::new("/tmp"), &mut names, &mut keys)?;
    Ok((names, keys))
}

fn collect_live_info_under(root: &Path, names: &mut HashSet<String>, keys: &mut HashSet<String>) -> Result<()> {
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
            keys.insert(crate::cmd::build::build_fallback::cwd_key(&path));
        }
        if path.starts_with(crate::worktree::aid_worktree_root()) {
            collect_live_info_under(&path, names, keys)?;
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
    use crate::test_env::CargoTargetDirGuard;
    use crate::test_subprocess;

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
        let _target_guard = CargoTargetDirGuard::set(&root);
        let live_branch = "feat/live-clean-target";
        let stale_target = root.join("feat-stale-clean-target");
        let live_target = root.join("feat-live-clean-target");
        let wt_path = Path::new("/tmp").join(format!("aid-wt-live-clean-target-{}", std::process::id()));
        let _ = fs::remove_dir_all(&wt_path);
        let _wt_guard = crate::test_env::TmpWorktreeGuard::with_repo(repo.path(), wt_path.clone());
        fs::create_dir_all(root.join(BASE_TARGET_DIR_NAME)).unwrap();
        fs::create_dir_all(&stale_target).unwrap();
        fs::create_dir_all(&live_target).unwrap();
        git(repo.path(), &["worktree", "add", &wt_path.to_string_lossy(), "-b", live_branch]);

        let temp_root = tempfile::tempdir().unwrap();
        let _fallback_guard = crate::test_env::FallbackTargetDirGuard::set(temp_root.path());
        let live_key = crate::cmd::build::build_fallback::cwd_key(&wt_path);
        let stale_key = crate::cmd::build::build_fallback::cwd_key(Path::new("/tmp/some-dead-wt"));
        let live_fallback = temp_root.path().join(live_key);
        let stale_fallback = temp_root.path().join(stale_key);
        fs::create_dir_all(&live_fallback).unwrap();
        fs::create_dir_all(&stale_fallback).unwrap();

        clean_orphaned_branch_targets(false, Some(temp_root.path())).unwrap();

        assert!(root.join(BASE_TARGET_DIR_NAME).exists());
        assert!(live_target.exists());
        assert!(!stale_target.exists());
        assert!(live_fallback.exists());
        assert!(!stale_fallback.exists());
    }
}
