// CLI handler for `aid clean` — removes old tasks, orphaned logs, and worktrees.
// Exports: run().
// Deps: crate::paths, crate::store::Store, chrono, rusqlite.

use anyhow::Result;
use chrono::{Duration, Local};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::paths;
use crate::store::Store;
use crate::worktree::{aid_worktree_root, is_aid_managed_worktree_path};

const ACTIVE_WORKTREES_SQL: &str = "SELECT DISTINCT worktree_path FROM tasks WHERE worktree_path IS NOT NULL AND status IN ('pending', 'running', 'awaiting_input')";
const TASK_IDS_SQL: &str = "SELECT id FROM tasks";
const WORKGROUP_IDS_SQL: &str = "SELECT id FROM workgroups";
const LOG_SUFFIX: &str = ".jsonl";

pub fn run(
    store: Arc<Store>,
    older_than_days: u64,
    clean_worktrees: bool,
    dry_run: bool,
) -> Result<()> {
    let _cutoff = Local::now() - Duration::days(older_than_days as i64);
    if dry_run {
        println!("[dry-run] Task records and events are retained as custody evidence");
    } else {
        println!("Task records and events retained as custody evidence");
    }
    if clean_worktrees {
        clean_orphaned_worktrees(&store, dry_run)?;
        crate::cmd::clean_cargo_target::clean_orphaned_branch_targets(dry_run, None)?;
    }
    clean_orphaned_logs(&store, dry_run)?;
    clean_orphaned_shared_dirs(&store, dry_run)?;
    Ok(())
}

fn clean_orphaned_worktrees(store: &Store, dry_run: bool) -> Result<()> {
    let _ = (store, dry_run);
    println!("Preserved orphaned worktree dirs; task artifacts require explicit acceptance and custody GC");
    Ok(())
}

fn clean_orphaned_logs(store: &Store, dry_run: bool) -> Result<()> {
    let task_ids = query_string_set(store, TASK_IDS_SQL)?;
    let mut removed = 0usize;
    for path in log_paths()? {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let task_id = name.trim_end_matches(LOG_SUFFIX);
        if task_ids.contains(task_id) {
            continue;
        }
        if dry_run {
            println!("[dry-run] Would remove orphaned log {}", path.display());
        } else {
            fs::remove_file(&path)?;
        }
        removed += 1;
    }
    println!(
        "{} {removed} orphaned logs",
        if dry_run {
            "[dry-run] Would remove"
        } else {
            "Removed"
        }
    );
    Ok(())
}

fn clean_orphaned_shared_dirs(store: &Store, dry_run: bool) -> Result<()> {
    let known_wgs = query_string_set(store, WORKGROUP_IDS_SQL)?;
    let shared_base = crate::paths::aid_dir().join("shared");
    if !shared_base.exists() {
        return Ok(());
    }
    let mut removed = 0usize;
    for entry in fs::read_dir(&shared_base)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let wg_id = entry.file_name().to_string_lossy().into_owned();
        if known_wgs.contains(&wg_id) {
            continue;
        }
        if dry_run {
            println!("[dry-run] Would remove orphaned shared dir {}", entry.path().display());
        } else {
            crate::shared_dir::cleanup_shared_dir(&wg_id);
            println!("Removed orphaned shared dir {}", entry.path().display());
        }
        removed += 1;
    }
    println!(
        "{} {removed} orphaned shared dirs",
        if dry_run { "[dry-run] Would remove" } else { "Removed" }
    );
    Ok(())
}

fn query_string_set(store: &Store, sql: &str) -> Result<HashSet<String>> {
    let conn = store.db();
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<HashSet<_>>>()?)
}

fn worktree_paths() -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_aid_home_worktree_paths(&aid_worktree_root(), &mut paths)?;
    collect_legacy_tmp_worktree_paths(Path::new("/tmp"), &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_aid_home_worktree_paths(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        if path.join(".git").is_file() && is_aid_managed_worktree_path(&path) {
            paths.push(path);
        } else {
            collect_aid_home_worktree_paths(&path, paths)?;
        }
    }
    Ok(())
}

fn collect_legacy_tmp_worktree_paths(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        if is_aid_managed_worktree_path(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

fn log_paths() -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(paths::logs_dir())? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("t-") && name.ends_with(LOG_SUFFIX) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains_path(paths: &[PathBuf], needle: &Path) -> bool {
        paths.iter().any(|path| path == needle)
    }

    #[test]
    fn legacy_tmp_worktree_path_is_collectable() {
        let worktree = tempfile::Builder::new()
            .prefix("aid-wt-clean-legacy-")
            .tempdir_in("/tmp")
            .unwrap();
        let mut paths = Vec::new();

        collect_legacy_tmp_worktree_paths(Path::new("/tmp"), &mut paths).unwrap();

        assert!(contains_path(&paths, worktree.path()));
    }

    #[test]
    fn non_aid_tmp_path_is_rejected_by_clean_scan() {
        let worktree = tempfile::Builder::new()
            .prefix("not-aid-clean-")
            .tempdir_in("/tmp")
            .unwrap();
        let mut paths = Vec::new();

        collect_legacy_tmp_worktree_paths(Path::new("/tmp"), &mut paths).unwrap();

        assert!(!contains_path(&paths, worktree.path()));
        assert!(!is_aid_managed_worktree_path(worktree.path()));
    }

    #[test]
    fn clean_orphaned_shared_dirs_removes_unknown_workgroup_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());
        let store = Store::open_memory().unwrap();
        crate::shared_dir::create_shared_dir("wg-orphanned").unwrap();
        crate::shared_dir::create_shared_dir("wg-known").unwrap();
        store.create_workgroup("Known", "", None, Some("wg-known")).unwrap();

        clean_orphaned_shared_dirs(&store, false).unwrap();

        assert!(crate::shared_dir::shared_dir_path("wg-orphanned").is_none());
        assert!(crate::shared_dir::shared_dir_path("wg-known").is_some());
    }
}
