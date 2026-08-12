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

pub fn get_dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    total += get_dir_size(&entry.path());
                } else if file_type.is_file() {
                    if let Ok(meta) = entry.metadata() {
                        total += meta.len();
                    }
                }
            }
        }
    }
    total
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.1} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}


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
    
    let mut total_bytes = 0;
    if clean_worktrees {
        total_bytes += clean_orphaned_worktrees(&store, dry_run)?;
        total_bytes += crate::cmd::clean_cargo_target::clean_orphaned_branch_targets(dry_run, None)?;
        total_bytes += clean_isolated_task_homes(&store, dry_run)?;
    }
    total_bytes += clean_orphaned_logs(&store, dry_run)?;
    total_bytes += clean_orphaned_shared_dirs(&store, dry_run)?;
    
    if total_bytes > 0 || dry_run {
        println!("---");
        println!("Total space {}: {}", if dry_run { "reclaimable" } else { "reclaimed" }, format_bytes(total_bytes));
    } else if !clean_worktrees {
        // Evaluate hint for worktrees if they didn't pass --worktrees
        let (cargo_bytes, fallback_bytes, home_bytes) = crate::cmd::clean_cargo_target::measure_orphaned_space(&store)?;
        let reclaimable = cargo_bytes + fallback_bytes + home_bytes;
        if reclaimable > 1024 * 1024 * 500 { // 500 MB threshold
            println!("---");
            println!("Hint: {} of reclaimable space is occupied by orphaned tasks. Run `aid clean --worktrees` to reclaim.", format_bytes(reclaimable));
        }
    }
    
    Ok(())
}

fn clean_orphaned_worktrees(store: &Store, dry_run: bool) -> Result<u64> {
    let _ = (store, dry_run);
    println!("Preserved orphaned worktree dirs; task artifacts require explicit acceptance and custody GC");
    Ok(0)
}

fn clean_orphaned_logs(store: &Store, dry_run: bool) -> Result<u64> {
    let task_ids = query_string_set(store, TASK_IDS_SQL)?;
    let mut removed = 0usize;
    let mut bytes = 0u64;
    for path in log_paths()? {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let task_id = name.trim_end_matches(LOG_SUFFIX);
        if task_ids.contains(task_id) {
            continue;
        }
        let size = path.metadata().map(|m| m.len()).unwrap_or(0);
        if dry_run {
            println!("[dry-run] Would remove orphaned log {} ({})", path.display(), format_bytes(size));
            bytes += size;
        } else {
            if fs::remove_file(&path).is_ok() {
                bytes += size;
            }
        }
        removed += 1;
    }
    if removed > 0 || dry_run {
        println!(
            "{} {removed} orphaned logs ({})",
            if dry_run { "[dry-run] Would remove" } else { "Removed" },
            format_bytes(bytes)
        );
    }
    Ok(bytes)
}

fn clean_orphaned_shared_dirs(store: &Store, dry_run: bool) -> Result<u64> {
    let known_wgs = query_string_set(store, WORKGROUP_IDS_SQL)?;
    let shared_base = crate::paths::aid_dir().join("shared");
    if !shared_base.exists() {
        return Ok(0);
    }
    let mut removed = 0usize;
    let mut bytes = 0u64;
    for entry in fs::read_dir(&shared_base)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let wg_id = entry.file_name().to_string_lossy().into_owned();
        if known_wgs.contains(&wg_id) {
            continue;
        }
        let size = get_dir_size(&entry.path());
        if dry_run {
            println!("[dry-run] Would remove orphaned shared dir {} ({})", entry.path().display(), format_bytes(size));
            bytes += size;
        } else {
            crate::shared_dir::cleanup_shared_dir(&wg_id);
            println!("Removed orphaned shared dir {} ({})", entry.path().display(), format_bytes(size));
            bytes += size;
        }
        removed += 1;
    }
    if removed > 0 || dry_run {
        println!(
            "{} {removed} orphaned shared dirs ({})",
            if dry_run { "[dry-run] Would remove" } else { "Removed" },
            format_bytes(bytes)
        );
    }
    Ok(bytes)
}

pub(crate) fn clean_isolated_task_homes(store: &Store, dry_run: bool) -> Result<u64> {
    let mut bytes = 0u64;
    let mut removed = 0usize;
    for id in crate::cmd::clean_cargo_target::orphaned_task_ids(store)? {
        let home_dir = crate::paths::task_dir(&id).join("home");
        if home_dir.exists() {
            let size = get_dir_size(&home_dir);
            if dry_run {
                println!("[dry-run] Would remove isolated task home for {} ({})", id, format_bytes(size));
                bytes += size;
            } else {
                if fs::remove_dir_all(&home_dir).is_ok() {
                    println!("Removed isolated task home for {} ({})", id, format_bytes(size));
                    bytes += size;
                }
            }
            removed += 1;
        }
    }
    if removed > 0 || dry_run {
        println!(
            "{} {removed} isolated task homes ({})",
            if dry_run { "[dry-run] Would remove" } else { "Removed" },
            format_bytes(bytes)
        );
    }
    Ok(bytes)
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
#[path = "clean_tests.rs"]
mod tests;
