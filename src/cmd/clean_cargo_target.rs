// Cargo target cleanup for `aid clean --worktrees`.
// Exports task-owned target discovery, cleanup, and bounded hint measurement.
// Deps: Store task records, agent target layout, build fallback paths, std fs.

use anyhow::Result;
use crate::store::Store;
use crate::types::{Task, TaskFilter};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const BASE_TARGET_DIR_NAME: &str = "_base";

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

pub(crate) fn clean_orphaned_branch_targets(
    store: &Store,
    dry_run: bool,
    fallback_root: Option<&Path>,
    sizes: &mut crate::cmd::clean_size::SizeTracker,
) -> Result<u64> {
    let targets = owned_target_dirs(store, fallback_root)?;
    let mut bytes = 0;
    let mut removed = 0;
    for (kind, target) in targets {
        let size = sizes.get_dir_size(&target)?;
        if dry_run {
            println!(
                "[dry-run] Would remove terminal task-owned {} {} ({})",
                kind.label(), target.display(), crate::cmd::clean::format_bytes(size)
            );
            bytes += size;
            removed += 1;
            continue;
        }
        if fs::remove_dir_all(&target).is_ok() {
            println!(
                "Removed terminal task-owned {} {} ({})",
                kind.label(), target.display(), crate::cmd::clean::format_bytes(size)
            );
            bytes += size;
            removed += 1;
        }
    }
    if removed > 0 || dry_run {
        println!(
            "{} {removed} terminal task-owned target dirs ({})",
            if dry_run { "[dry-run] Would remove" } else { "Removed" },
            crate::cmd::clean::format_bytes(bytes)
        );
    }
    Ok(bytes)
}

pub(crate) fn has_reclaimable_space_above_threshold(store: &Store) -> Result<Option<u64>> {
    let mut sizes = crate::cmd::clean_size::SizeTracker::new();
    let mut bytes = 0;
    let mut entries = 0;
    for path in owned_target_dirs(store, None)?
        .into_iter()
        .map(|(_, path)| path)
        .chain(terminal_task_homes(store)?.into_iter())
    {
        if bytes >= crate::cmd::clean_size::CLEANUP_HINT_BYTE_LIMIT
            || entries >= crate::cmd::clean_size::CLEANUP_HINT_ENTRY_LIMIT
        {
            break;
        }
        let (measured, scanned) = sizes.get_dir_size_bounded(
            &path,
            crate::cmd::clean_size::CLEANUP_HINT_BYTE_LIMIT - bytes,
            crate::cmd::clean_size::CLEANUP_HINT_ENTRY_LIMIT - entries,
        )?;
        bytes += measured;
        entries += scanned;
    }
    if bytes >= crate::cmd::clean_size::CLEANUP_HINT_THRESHOLD_BYTES {
        Ok(Some(bytes))
    } else {
        Ok(None)
    }
}

pub(crate) fn terminal_task_ids(store: &Store) -> Result<Vec<String>> {
    Ok(store
        .list_tasks(TaskFilter::All)?
        .into_iter()
        .filter(|task| task.status.is_terminal())
        .map(|task| task.id.0)
        .collect())
}

fn terminal_task_homes(store: &Store) -> Result<Vec<PathBuf>> {
    Ok(terminal_task_ids(store)?
        .into_iter()
        .map(|id| crate::paths::task_dir(&id).join("home"))
        .filter(|path| path.is_dir())
        .collect())
}

fn owned_target_dirs(store: &Store, fallback_root: Option<&Path>) -> Result<Vec<(TargetKind, PathBuf)>> {
    let tasks = store.list_tasks(TaskFilter::All)?;
    let mut terminal = Vec::new();
    let mut blocked = HashSet::new();
    let branch_root = crate::agent::env::branch_target_root();
    let fallback_root = fallback_root
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::cmd::build::build_fallback::fallback_target_root);

    for task in tasks {
        let paths = task_target_paths(&task, branch_root.as_deref(), &fallback_root);
        if task.status.is_terminal() && !has_live_worktree(&task) {
            terminal.extend(paths.iter().cloned());
        } else {
            blocked.extend(paths.iter().map(|(_, path)| path.clone()));
        }
    }
    let mut seen = HashSet::new();
    terminal.retain(|(_, path)| blocked.contains(path) == false && seen.insert(path.clone()) && path.is_dir());
    Ok(terminal)
}

fn task_target_paths(
    task: &Task,
    branch_root: Option<&Path>,
    fallback_root: &Path,
) -> Vec<(TargetKind, PathBuf)> {
    let mut paths = Vec::new();
    if let (Some(root), Some(branch)) = (
        branch_root,
        task.worktree_branch.as_deref().filter(|branch| !branch.trim().is_empty()),
    ) {
        let name = crate::agent::env::branch_target_name(branch);
        if !is_reserved_target_dir_name(&name) {
            let target = (root.file_name().and_then(|value| value.to_str()) == Some(name.as_str()))
                .then(|| root.to_path_buf())
                .unwrap_or_else(|| root.join(name));
            paths.push((TargetKind::Branch, target));
        }
    }
    if let Some(cwd) = task.worktree_path.as_deref().or(task.repo_path.as_deref()) {
        paths.push((
            TargetKind::Fallback,
            fallback_root.join(crate::cmd::build::build_fallback::cwd_key(Path::new(cwd))),
        ));
    }
    paths
}

fn has_live_worktree(task: &Task) -> bool {
    task.worktree_branch.is_some()
        && task
            .worktree_path
            .as_deref()
            .is_some_and(|path| Path::new(path).is_dir())
}

fn is_reserved_target_dir_name(name: &str) -> bool {
    name == BASE_TARGET_DIR_NAME
        || name.starts_with('.')
        || matches!(name, "debug" | "release" | "tmp" | "doc" | "package")
}

#[cfg(test)]
#[path = "clean_cargo_target_tests.rs"]
mod tests;
