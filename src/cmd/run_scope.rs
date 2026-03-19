// Scope validation helpers for `aid run`.
// Exports path normalization and changed-file scope warnings for committed work.
use std::path::{Component, Path, PathBuf};

use crate::worktree;

use super::resolve_repo_path;

pub(in crate::cmd) fn warn_agent_committed_files_outside_scope(
    scope: &[String],
    dir: Option<&String>,
    effective_dir: Option<&String>,
    resolved_repo: Option<&String>,
    worktree_path: Option<&String>,
) {
    if scope.is_empty() && dir.map(|value| value.trim()).unwrap_or("").is_empty() {
        return;
    }
    let base_path = worktree_path
        .map(PathBuf::from)
        .or_else(|| effective_dir.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let changed_files = match worktree::worktree_changed_files(&base_path) {
        Ok(files) if !files.is_empty() => files,
        _ => return,
    };
    let base_dir = base_path.to_string_lossy().to_string();
    let repo_root = resolved_repo
        .map(PathBuf::from)
        .or_else(|| resolve_repo_path(&base_dir).ok().map(PathBuf::from));
    let scope_paths = normalized_scope_paths(scope, repo_root.as_deref());
    let dir_path = normalized_dir_path(dir, repo_root.as_deref());
    if scope_paths.is_empty() && dir_path.is_none() {
        return;
    }
    let mut violations = Vec::new();
    for file in changed_files {
        let file_path = Path::new(&file);
        let scope_violation = !scope_paths.is_empty()
            && !scope_paths
                .iter()
                .any(|scope| file_path == scope || file_path.starts_with(scope));
        let dir_violation = dir_path
            .as_ref()
            .is_some_and(|dir| !(file_path == dir || file_path.starts_with(dir)));
        if scope_violation || dir_violation {
            violations.push(file);
        }
    }
    if violations.is_empty() {
        return;
    }
    aid_warn!(
        "[aid] Warning: agent committed {} files outside scope: {:?}",
        violations.len(),
        violations
    );
}

fn normalized_scope_paths(scope: &[String], repo_root: Option<&Path>) -> Vec<PathBuf> {
    scope
        .iter()
        .filter_map(|entry| {
            let trimmed = entry.trim().trim_end_matches('/');
            if trimmed.is_empty() {
                return None;
            }
            let path = Path::new(trimmed);
            let relative = if path.is_absolute() {
                let root = repo_root?;
                path.strip_prefix(root).ok()?
            } else {
                path
            };
            let normalized = normalize_relative_path(relative);
            if normalized.as_os_str().is_empty() {
                return None;
            }
            Some(normalized)
        })
        .collect()
}

fn normalized_dir_path(dir: Option<&String>, repo_root: Option<&Path>) -> Option<PathBuf> {
    let dir = dir?;
    let trimmed = dir.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return None;
    }
    let path = Path::new(trimmed);
    let relative = if path.is_absolute() {
        let root = repo_root?;
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let normalized = normalize_relative_path(relative);
    if normalized.as_os_str().is_empty() {
        return None;
    }
    Some(normalized)
}

fn normalize_relative_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
