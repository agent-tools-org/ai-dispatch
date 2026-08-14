// Cargo target cleanup for `aid clean --worktrees` and custody GC.
// Exports task-owned target discovery, liveness verification, and cleanup.
// Deps: Store task records, agent target layout, build fallback paths, std fs.

use anyhow::Result;
use crate::store::Store;
use crate::types::{Task, TaskFilter};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const BASE_TARGET_DIR_NAME: &str = "_base";
const RECENT_MODIFICATION_THRESHOLD_SECS: u64 = 30;

#[derive(Debug, Clone, Copy)]
enum TargetKind {
    Branch,
    Fallback,
}

impl TargetKind {
    fn label(self) -> &'static str {
        match self {
            Self::Branch => "Cargo target",
            Self::Fallback => "fallback target",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TargetLiveness {
    Idle,
    Live(String),
}

pub(crate) fn check_target_dir_liveness(target: &Path) -> TargetLiveness {
    let lock_file = target.join(".cargo-lock");
    if lock_file.exists() {
        match try_lock_cargo_lock(&lock_file) {
            Ok(true) => TargetLiveness::Idle,
            Ok(false) => TargetLiveness::Live(".cargo-lock is held by an active build".to_string()),
            Err(e) => TargetLiveness::Live(format!("cannot verify .cargo-lock: {e}")),
        }
    } else {
        check_recent_modification(target)
    }
}

#[cfg(unix)]
fn try_lock_cargo_lock(lock_path: &Path) -> std::io::Result<bool> {
    use std::os::unix::io::AsRawFd;
    let file = fs::File::open(lock_path)?;
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        Ok(true)
    } else {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) || err.raw_os_error() == Some(libc::EAGAIN) {
            Ok(false)
        } else {
            Err(err)
        }
    }
}

#[cfg(not(unix))]
fn try_lock_cargo_lock(_lock_path: &Path) -> std::io::Result<bool> {
    Ok(true)
}

fn check_recent_modification(target: &Path) -> TargetLiveness {
    let threshold = Duration::from_secs(RECENT_MODIFICATION_THRESHOLD_SECS);
    let mut newest = fs::metadata(target).and_then(|m| m.modified()).ok();
    if let Ok(entries) = fs::read_dir(target) {
        for entry in entries.flatten() {
            if let Ok(m) = entry.metadata().and_then(|m| m.modified()) {
                newest = Some(newest.map_or(m, |prev| prev.max(m)));
            }
        }
    }
    if let Some(mtime) = newest {
        let age = SystemTime::now().duration_since(mtime).unwrap_or(Duration::ZERO);
        if age < threshold {
            return TargetLiveness::Live(format!("target modified recently ({}s ago, threshold {}s)", age.as_secs(), threshold.as_secs()));
        }
    }
    TargetLiveness::Idle
}

pub(crate) fn clean_orphaned_branch_targets(
    store: &Store,
    dry_run: bool,
    fallback_root: Option<&Path>,
    sizes: &mut crate::cmd::clean_size::SizeTracker,
) -> Result<u64> {
    let targets = owned_target_dirs(store, fallback_root)?;
    let (mut bytes, mut removed) = (0, 0);
    for (kind, target) in targets {
        if !is_safe_target_for_removal(&target) { continue; }
        if let TargetLiveness::Live(reason) = check_target_dir_liveness(&target) {
            println!("{}Skipped live {} {}: {reason}", if dry_run { "[dry-run] " } else { "" }, kind.label(), target.display());
            continue;
        }
        let size = sizes.get_dir_size(&target)?;
        if dry_run {
            println!("[dry-run] Would remove terminal task-owned {} {} ({})", kind.label(), target.display(), crate::cmd::clean::format_bytes(size));
            bytes += size;
            removed += 1;
        } else if fs::remove_dir_all(&target).is_ok() {
            println!("Removed terminal task-owned {} {} ({})", kind.label(), target.display(), crate::cmd::clean::format_bytes(size));
            bytes += size;
            removed += 1;
        }
    }
    if removed > 0 || dry_run {
        println!("{} {removed} terminal task-owned target dirs ({})", if dry_run { "[dry-run] Would remove" } else { "Removed" }, crate::cmd::clean::format_bytes(bytes));
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReclaimableSpaceEstimate {
    pub(crate) bytes: u64,
    pub(crate) truncated: bool,
}

pub(crate) fn has_reclaimable_space_above_threshold(store: &Store) -> Result<Option<ReclaimableSpaceEstimate>> {
    has_reclaimable_space_above_threshold_with_entry_limit(store, crate::cmd::clean_size::CLEANUP_HINT_ENTRY_LIMIT)
}

fn has_reclaimable_space_above_threshold_with_entry_limit(store: &Store, entry_limit: usize) -> Result<Option<ReclaimableSpaceEstimate>> {
    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    let (mut bytes, mut entries, mut truncated) = (0, 0, false);
    for path in owned_target_dirs(store, None)?.into_iter().map(|(_, p)| p).chain(terminal_task_homes(store)?) {
        if bytes >= crate::cmd::clean_size::CLEANUP_HINT_BYTE_LIMIT || entries >= entry_limit {
            truncated = true;
            break;
        }
        let scan = scan_hint_path(&mut sizes, &path, crate::cmd::clean_size::CLEANUP_HINT_BYTE_LIMIT - bytes, entry_limit - entries)?;
        bytes += scan.bytes;
        entries += scan.entries;
        if scan.truncated {
            truncated = true;
            break;
        }
    }
    if truncated || bytes >= crate::cmd::clean_size::CLEANUP_HINT_THRESHOLD_BYTES {
        Ok(Some(ReclaimableSpaceEstimate { bytes, truncated }))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HintScan {
    bytes: u64,
    entries: usize,
    truncated: bool,
}

fn scan_hint_path(sizes: &mut crate::cmd::clean_size::SizeTracker, path: &Path, byte_limit: u64, entry_limit: usize) -> Result<HintScan> {
    let (bytes, entries) = sizes.get_dir_size_bounded(path, byte_limit, entry_limit)?;
    Ok(HintScan { bytes, entries, truncated: entries >= entry_limit || bytes >= byte_limit })
}

pub(crate) fn terminal_task_ids(store: &Store) -> Result<Vec<String>> {
    Ok(store.list_tasks(TaskFilter::All)?.into_iter().filter(|t| t.status.is_terminal()).map(|t| t.id.0).collect())
}

fn terminal_task_homes(store: &Store) -> Result<Vec<PathBuf>> {
    Ok(terminal_task_ids(store)?.into_iter().map(|id| crate::paths::task_dir(&id).join("home")).filter(|p| p.is_dir()).collect())
}

fn owned_target_dirs(store: &Store, fallback_root: Option<&Path>) -> Result<Vec<(TargetKind, PathBuf)>> {
    let tasks = store.list_tasks(TaskFilter::All)?;
    let mut terminal_candidates = Vec::new();
    let mut blocked = HashSet::new();
    let branch_root = crate::agent::env::branch_target_root();
    let fallback_root = fallback_root.map(Path::to_path_buf).unwrap_or_else(crate::cmd::build::build_fallback::fallback_target_root);

    for task in &tasks {
        let paths = task_target_paths(task, branch_root.as_deref(), &fallback_root);
        if task.status.is_terminal() && !has_live_worktree(task) {
            terminal_candidates.extend(paths);
        } else {
            blocked.extend(paths.into_iter().map(|(_, path)| path));
        }
    }

    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for (kind, path) in terminal_candidates {
        if blocked.contains(&path) || !seen.insert(path.clone()) || !path.is_dir() {
            continue;
        }
        let safe = match kind {
            TargetKind::Branch => is_safe_target_for_removal(&path),
            TargetKind::Fallback => is_safe_fallback_target_for_removal(&path, &fallback_root),
        };
        if safe {
            result.push((kind, path));
        }
    }
    Ok(result)
}

pub(crate) fn remove_task_fallback_target_dirs(store: &Store, task: &Task) -> Result<u64> {
    let fallback_root = crate::cmd::build::build_fallback::fallback_target_root();
    if !fallback_root.is_dir() {
        return Ok(0);
    }
    let tasks = store.list_tasks(TaskFilter::All)?;
    let mut blocked = HashSet::new();
    let branch_root = crate::agent::env::branch_target_root();
    for other in &tasks {
        if other.id != task.id && (!other.status.is_terminal() || has_live_worktree(other)) {
            for (_, path) in task_target_paths(other, branch_root.as_deref(), &fallback_root) {
                blocked.insert(path);
            }
        }
    }
    let mut reclaimed = 0;
    for (_, path) in task_target_paths(task, branch_root.as_deref(), &fallback_root) {
        if is_safe_fallback_target_for_removal(&path, &fallback_root) && !blocked.contains(&path) {
            if let TargetLiveness::Live(reason) = check_target_dir_liveness(&path) {
                println!("Skipped live fallback target {}: {reason}", path.display());
                continue;
            }
            let size = crate::cmd::clean_size::get_dir_size(&path).unwrap_or(0);
            if fs::remove_dir_all(&path).is_ok() {
                reclaimed += size;
            }
        }
    }
    Ok(reclaimed)
}

fn task_target_paths(task: &Task, branch_root: Option<&Path>, fallback_root: &Path) -> Vec<(TargetKind, PathBuf)> {
    let mut paths = Vec::new();
    if let (Some(root), Some(branch)) = (branch_root, task.worktree_branch.as_deref().filter(|b| !b.trim().is_empty())) {
        let name = crate::agent::env::branch_target_name(branch);
        if !is_reserved_target_dir_name(&name) {
            paths.push((TargetKind::Branch, root.join(name)));
        }
    }
    for cwd in [task.worktree_path.as_deref(), task.repo_path.as_deref(), task.effective_dir.as_deref()].into_iter().flatten() {
        paths.push((TargetKind::Fallback, fallback_root.join(crate::cmd::build::build_fallback::cwd_key(Path::new(cwd)))));
    }
    paths
}

fn is_safe_fallback_target_for_removal(path: &Path, fallback_root: &Path) -> bool {
    if !path.is_dir() || path.parent() != Some(fallback_root) {
        return false;
    }
    if fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(true) {
        return false;
    }
    is_safe_target_for_removal(path)
}

fn is_safe_target_for_removal(target: &Path) -> bool {
    if crate::agent::env::branch_target_root().is_some_and(|root| root == target) {
        return false;
    }
    if std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).is_some_and(|root| root == target) {
        return false;
    }
    true
}

fn has_live_worktree(task: &Task) -> bool {
    task.worktree_branch.is_some() && task.worktree_path.as_deref().is_some_and(|path| Path::new(path).is_dir())
}

fn is_reserved_target_dir_name(name: &str) -> bool {
    name == BASE_TARGET_DIR_NAME || name.starts_with('.') || matches!(name, "debug" | "release" | "tmp" | "doc" | "package")
}

#[cfg(test)]
#[path = "clean_cargo_target_tests.rs"]
mod tests;
