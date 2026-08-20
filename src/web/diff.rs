// Lightweight task diff presence checks for fleet payload enrichment.
// Exports: has_non_empty_diff without issuing store queries per task.
// Deps: git, Task, and delivery assessment state.

use crate::types::{DeliveryAssessment, Task};
use std::path::Path;
use std::process::Command;

pub(crate) fn has_non_empty_diff(task: &Task) -> bool {
    if matches!(task.delivery_assessment, Some(DeliveryAssessment::EmptyDiff)) {
        return false;
    }
    let Some(repository) = task.worktree_path.as_deref().or(task.repo_path.as_deref()) else {
        return false;
    };
    if !Path::new(repository).is_dir() {
        return false;
    }
    if let Some(start_sha) = task.start_sha.as_deref() {
        return git_diff_changed(repository, &[start_sha, "--"]);
    }
    git_diff_changed(repository, &["--"])
}

fn git_diff_changed(repository: &str, args: &[&str]) -> bool {
    Command::new("git")
        .args(["-C", repository, "diff", "--quiet"])
        .args(args)
        .status()
        .map(|status| status.code() == Some(1))
        .unwrap_or(false)
}
