// Stable project identity resolution for dispatch, board filtering, and display.
// Exports: resolve_project_id, path_based_project_id, project_display, filter helpers.
// Deps: detect_project_in, git CLI for main working-tree discovery.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::detect_project_in;

/// UI / filter label for tasks with `project_id IS NULL`.
/// Historical rows that never recorded a project stay here — never invented.
pub const UNATTRIBUTED: &str = "unattributed";

/// Resolve the stable project identity for a working directory.
///
/// Identity is **not** session or cwd:
/// 1. Find the enclosing git repo and its **main working tree** so a main
///    checkout and its linked worktrees share one identity.
/// 2. Prefer `[project].id` from `.aid/project.toml` on that main tree
///    (declared, portable when operators set the same id).
/// 3. Else fall back to `basename-<hash8>` of the main tree's canonical path
///    (same scheme as aid worktree layout).
/// 4. Outside any git repository → `None` (stored as SQL NULL / unattributed).
pub fn resolve_project_id(start_dir: &Path) -> Option<String> {
    let main = main_working_tree(start_dir)?;
    if let Some(id) = project_toml_id(&main) {
        return Some(id);
    }
    // Worktree checkouts may carry their own project.toml copy.
    if let Some(local_root) = git_toplevel(start_dir) {
        if local_root != main {
            if let Some(id) = project_toml_id(&local_root) {
                return Some(id);
            }
        }
    }
    Some(path_based_project_id(&main))
}

/// Current process identity: cwd-derived, but via main-tree rules above.
pub fn current_project_id() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    resolve_project_id(&cwd)
}

/// Prefer an already-loaded config id; otherwise resolve from `start_dir`.
pub fn resolve_project_id_with_config(
    config_id: Option<&str>,
    start_dir: &Path,
) -> Option<String> {
    if let Some(id) = config_id.map(str::trim).filter(|id| !id.is_empty()) {
        return Some(id.to_string());
    }
    resolve_project_id(start_dir)
}

/// Display label: known id, or the explicit unattributed bucket.
pub fn project_display(project_id: Option<&str>) -> &str {
    match project_id.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => id,
        None => UNATTRIBUTED,
    }
}

/// Keep only tasks that belong to `filter`.
/// `filter = None` means the unattributed bucket (SQL NULL), not "all projects".
pub fn retain_project(tasks: &mut Vec<crate::types::Task>, filter: Option<&str>) {
    match filter {
        Some(id) => tasks.retain(|task| task.project_id.as_deref() == Some(id)),
        None => tasks.retain(|task| task.project_id.is_none()),
    }
}

/// Whether `task` matches an active project filter.
pub fn matches_project_filter(task_project_id: Option<&str>, filter: Option<&str>) -> bool {
    match filter {
        Some(id) => task_project_id == Some(id),
        None => task_project_id.is_none(),
    }
}

/// Banner text when a project filter is active (never silent).
pub fn project_filter_banner(filter: Option<&str>, all_projects: bool) -> String {
    if all_projects {
        return "project:* (all projects; default is current project)".to_string();
    }
    format!(
        "project:{} (use --all to show every project)",
        project_display(filter)
    )
}

/// Path-based fallback: `basename-<8 hex chars of DefaultHasher(canonical path)>`.
pub fn path_based_project_id(repo_dir: &Path) -> String {
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
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            path.to_string_lossy().hash(&mut hasher);
            format!("{:x}", hasher.finish())
        })
        .unwrap_or_else(|| "0".to_string());
    let hash_short: String = hash.chars().take(8).collect();
    format!("{basename}-{hash_short}")
}

fn project_toml_id(repo_dir: &Path) -> Option<String> {
    let config = detect_project_in(repo_dir)?;
    let id = config.id.trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn main_working_tree(start_dir: &Path) -> Option<PathBuf> {
    let toplevel = git_toplevel(start_dir)?;
    main_working_tree_of(&toplevel).or(Some(toplevel))
}

fn git_toplevel(start_dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["-C", &start_dir.to_string_lossy()])
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        None
    } else {
        Some(PathBuf::from(raw))
    }
}

fn main_working_tree_of(repo_dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "list", "--porcelain"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(path) = line.strip_prefix("worktree ") else {
            continue;
        };
        let path = path.trim();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    None
}

#[cfg(test)]
#[path = "identity_tests.rs"]
mod tests;
