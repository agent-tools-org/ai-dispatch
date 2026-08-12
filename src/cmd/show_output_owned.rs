// Task-owned output path resolution for `aid show --output`.
// Relative `-o` paths resolve only under this task's worktree/repo/task dir — never CWD.
// Exports: owned_output_path, missing_owned_output_notice (via show_output_messages).
// Deps: paths, Task.

use std::path::{Path, PathBuf};

use crate::paths;
use crate::types::Task;

/// Resolve the declared `-o` path to a file proven to belong to this task.
///
/// - Absolute paths: accepted only as the exact path recorded on the task (declared ownership).
/// - Relative paths: joined only with worktree, repo, then task_dir. Process CWD is never used.
pub(crate) fn owned_output_path(task: &Task) -> Option<PathBuf> {
    let declared = task.output_path.as_deref()?;
    let path = Path::new(declared);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    for base in task_output_bases(task) {
        let candidate = base.join(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn task_output_bases(task: &Task) -> Vec<PathBuf> {
    let mut bases = Vec::with_capacity(3);
    if let Some(wt) = task.worktree_path.as_deref() {
        bases.push(PathBuf::from(wt));
    }
    if let Some(repo) = task.repo_path.as_deref() {
        let repo = PathBuf::from(repo);
        if bases.iter().all(|base| base != &repo) {
            bases.push(repo);
        }
    }
    let task_dir = paths::task_dir(task.id.as_str());
    if bases.iter().all(|base| base != &task_dir) {
        bases.push(task_dir);
    }
    bases
}

/// Explicit absence when the task declared `-o` but no owned file exists.
pub(super) fn missing_owned_output_notice(task: &Task) -> Option<String> {
    let declared = task.output_path.as_deref()?;
    if owned_output_path(task).is_some() {
        return None;
    }
    Some(format!(
        "No task-owned output file for this task (declared: {declared}). Falling back to this task's log.\n"
    ))
}
