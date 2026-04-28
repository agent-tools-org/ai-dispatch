// CLI handler for `aid clean` — removes old tasks, orphaned logs, and worktrees.
// Exports: run().
// Deps: crate::paths, crate::store::Store, chrono, rusqlite.

use anyhow::Result;
use chrono::{Duration, Local};
use rusqlite::params;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::paths;
use crate::store::Store;
use crate::worktree::{aid_worktree_root, is_aid_managed_worktree_path};

const COUNT_OLD_TASKS_SQL: &str = "SELECT COUNT(*) FROM tasks WHERE status IN ('done', 'failed', 'merged', 'skipped') AND created_at < ?1";
const DELETE_OLD_EVENTS_SQL: &str = "DELETE FROM events WHERE task_id IN (SELECT id FROM tasks WHERE status IN ('done', 'failed', 'merged', 'skipped') AND created_at < ?1)";
const DELETE_OLD_TASKS_SQL: &str =
    "DELETE FROM tasks WHERE status IN ('done', 'failed', 'merged', 'skipped') AND created_at < ?1";
const ACTIVE_WORKTREES_SQL: &str = "SELECT DISTINCT worktree_path FROM tasks WHERE worktree_path IS NOT NULL AND status IN ('pending', 'running', 'awaiting_input')";
const TASK_IDS_SQL: &str = "SELECT id FROM tasks";
const LOG_SUFFIX: &str = ".jsonl";

pub fn run(
    store: Arc<Store>,
    older_than_days: u64,
    clean_worktrees: bool,
    dry_run: bool,
) -> Result<()> {
    let cutoff_str = (Local::now() - Duration::days(older_than_days as i64)).to_rfc3339();
    let task_count = count_old_tasks(&store, &cutoff_str)?;
    if dry_run {
        println!("[dry-run] Would delete {task_count} tasks older than {older_than_days} days");
    } else {
        let (tasks_deleted, events_deleted) = delete_old_tasks(&store, &cutoff_str)?;
        println!("Cleaned {tasks_deleted} tasks and {events_deleted} events older than {older_than_days} days");
    }
    if clean_worktrees {
        clean_orphaned_worktrees(&store, dry_run)?;
    }
    clean_orphaned_logs(&store, dry_run)?;
    Ok(())
}

fn count_old_tasks(store: &Store, cutoff_str: &str) -> Result<i64> {
    let conn = store.db();
    Ok(conn.query_row(COUNT_OLD_TASKS_SQL, params![cutoff_str], |row| row.get(0))?)
}

fn delete_old_tasks(store: &Store, cutoff_str: &str) -> Result<(usize, usize)> {
    let conn = store.db();
    let events_deleted = conn.execute(DELETE_OLD_EVENTS_SQL, params![cutoff_str])?;
    let tasks_deleted = conn.execute(DELETE_OLD_TASKS_SQL, params![cutoff_str])?;
    Ok((tasks_deleted, events_deleted))
}

fn clean_orphaned_worktrees(store: &Store, dry_run: bool) -> Result<()> {
    let active_paths = query_string_set(store, ACTIVE_WORKTREES_SQL)?;
    let mut removed = 0usize;
    for path in worktree_paths()? {
        let path_str = path.to_string_lossy().into_owned();
        if active_paths.contains(&path_str) {
            continue;
        }
        if dry_run {
            println!(
                "[dry-run] Would remove orphaned worktree {}",
                path.display()
            );
        } else {
            // SANDBOX: double-check path is under aid-managed worktree roots before deletion.
            if !is_aid_managed_worktree_path(&path) {
                aid_warn!(
                    "[aid] SAFETY: refusing to remove '{}' — not an aid worktree",
                    path.display()
                );
                continue;
            }
            fs::remove_dir_all(&path)?;
            println!("Removed orphaned worktree {}", path.display());
        }
        removed += 1;
    }
    println!(
        "{} {removed} orphaned worktrees",
        if dry_run {
            "[dry-run] Would remove"
        } else {
            "Removed"
        }
    );
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
}
