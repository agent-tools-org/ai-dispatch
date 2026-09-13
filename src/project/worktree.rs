// Shared Git checkout discovery for project configuration and identity.
// Exports: main_working_tree, main_working_tree_of, git_toplevel within project.
// Deps: std::path, std::process::Command, Git CLI.

use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn main_working_tree(start_dir: &Path) -> Option<PathBuf> {
    let toplevel = git_toplevel(start_dir)?;
    main_working_tree_of(&toplevel).or(Some(toplevel))
}

pub(super) fn git_toplevel(start_dir: &Path) -> Option<PathBuf> {
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

pub(super) fn main_working_tree_of(repo_dir: &Path) -> Option<PathBuf> {
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
