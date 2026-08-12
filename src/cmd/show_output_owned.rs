// Task-owned output path resolution for `aid show --output`.
// Relative `-o` resolves under this task's recorded dir/worktree/task dir — never CWD or repo root.
// Exports: owned_output_path, missing_owned_output_notice, missing_owned_output_absence.
// Deps: paths, Task.

use std::path::{Path, PathBuf};

use crate::paths;
use crate::types::Task;

/// Resolve the declared `-o` path to a file proven to belong to this task.
///
/// - Absolute paths: accepted only as the exact path recorded on the task.
/// - Relative paths: joined only with this task's effective dir, worktree, then
///   task_dir. Never process CWD and never the shared repository root.
pub(crate) fn owned_output_path(task: &Task) -> Option<PathBuf> {
    let declared = task.output_path.as_deref()?;
    let path = Path::new(declared);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    for base in task_output_bases(task) {
        if let Some(candidate) = owned_file_under_base(&base, path) {
            return Some(candidate);
        }
    }
    None
}

fn owned_file_under_base(base: &Path, relative: &Path) -> Option<PathBuf> {
    let candidate = base.join(relative);
    let canon_base = base.canonicalize().ok()?;
    let canon_file = candidate.canonicalize().ok()?;
    if !canon_file.starts_with(&canon_base) || !canon_file.is_file() {
        return None;
    }
    Some(canon_file)
}

fn task_output_bases(task: &Task) -> Vec<PathBuf> {
    let mut bases = Vec::with_capacity(3);
    push_unique_base(&mut bases, task.effective_dir.as_deref());
    push_unique_base(&mut bases, task.worktree_path.as_deref());
    push_unique_base(&mut bases, Some(paths::task_dir(task.id.as_str())));
    bases
}

fn push_unique_base(bases: &mut Vec<PathBuf>, raw: Option<impl AsRef<Path>>) {
    let Some(raw) = raw else { return };
    let path = raw.as_ref().to_path_buf();
    if path.as_os_str().is_empty() || bases.iter().any(|base| base == &path) {
        return;
    }
    bases.push(path);
}

/// Declared `-o` when no owned file exists.
pub(crate) fn missing_owned_output_declared(task: &Task) -> Option<&str> {
    let declared = task.output_path.as_deref()?;
    owned_output_path(task).is_none().then_some(declared)
}

/// Explicit absence, without promising a particular fallback.
pub(crate) fn missing_owned_output_absence(task: &Task) -> Option<String> {
    missing_owned_output_declared(task).map(|declared| {
        format!("No task-owned output file for this task (declared: {declared}).")
    })
}

/// Explicit absence when `aid show --output` falls back to this task's log.
pub(super) fn missing_owned_output_notice(task: &Task) -> Option<String> {
    missing_owned_output_absence(task)
        .map(|absence| format!("{absence} Falling back to this task's log.\n"))
}
