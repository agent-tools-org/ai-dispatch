// Worktree snapshot reader for dirty-state and diff classification.
// Exports parsed status entries plus a single capture_worktree_snapshot boundary.
// Deps: git CLI via std::process, anyhow, std::path.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSnapshot {
    pub status_lines: Vec<String>,
    pub entries: Vec<WorktreeStatusEntry>,
    pub empty_diff: Option<bool>,
}

impl WorktreeSnapshot {
    pub fn has_uncommitted_changes(&self) -> bool {
        !self.status_lines.is_empty()
    }

    pub fn rescuable_entries(&self) -> Vec<WorktreeStatusEntry> {
        self.entries
            .iter()
            .filter(|entry| is_rescuable_path(&entry.path))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeStatusKind {
    Untracked,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeStatusEntry {
    pub path: String,
    pub kind: WorktreeStatusKind,
}

pub fn capture_worktree_snapshot(dir: &Path) -> Result<WorktreeSnapshot> {
    let status_lines = read_status_lines(dir)?;
    let entries = status_lines
        .iter()
        .filter_map(|line| parse_status_entry(line))
        .collect();
    Ok(WorktreeSnapshot {
        status_lines,
        entries,
        empty_diff: read_empty_diff(dir),
    })
}

pub fn parse_status_entry(line: &str) -> Option<WorktreeStatusEntry> {
    if let Some(path) = line.strip_prefix("?? ") {
        return Some(WorktreeStatusEntry {
            path: path.to_string(),
            kind: WorktreeStatusKind::Untracked,
        });
    }
    if line.len() < 4 {
        return None;
    }
    let status = &line[..2];
    if !status.contains('M') {
        return None;
    }
    Some(WorktreeStatusEntry {
        path: line[3..].to_string(),
        kind: WorktreeStatusKind::Modified,
    })
}

pub fn is_rescuable_path(path: &str) -> bool {
    if path.starts_with(".aid/")
        || (path.starts_with("result-t-") && path.ends_with(".md"))
    {
        return false;
    }
    !["target/", "node_modules/", "__pycache__/", ".aid-", "aid-batch-"]
        .iter()
        .any(|part| path.contains(part))
        && ![".pyc", ".pyo", ".class", ".o", ".so", ".dylib"]
            .iter()
            .any(|suffix| path.ends_with(suffix))
}

fn read_status_lines(dir: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
        .context("Failed to run git status")?;
    anyhow::ensure!(
        output.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

fn read_empty_diff(dir: &Path) -> Option<bool> {
    let head = git_diff_stat_output(dir, &["diff", "--stat", "HEAD"])?;
    let staged = git_diff_stat_output(dir, &["diff", "--cached", "--stat"])?;
    Some(head.trim().is_empty() && staged.trim().is_empty())
}

fn git_diff_stat_output(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::{WorktreeStatusKind, is_rescuable_path, parse_status_entry};

    #[test]
    fn worktree_snapshot_parses_status_entries() {
        let untracked = parse_status_entry("?? src/new.rs").unwrap();
        assert_eq!(untracked.path, "src/new.rs");
        assert_eq!(untracked.kind, WorktreeStatusKind::Untracked);

        let modified = parse_status_entry(" M src/lib.rs").unwrap();
        assert_eq!(modified.path, "src/lib.rs");
        assert_eq!(modified.kind, WorktreeStatusKind::Modified);

        assert!(parse_status_entry(" D src/lib.rs").is_none());
    }

    #[test]
    fn worktree_snapshot_filters_non_source_artifacts() {
        assert!(is_rescuable_path("src/lib.rs"));
        assert!(!is_rescuable_path("target/debug/app"));
        assert!(!is_rescuable_path("cache/file.pyc"));
    }

    #[test]
    fn is_rescuable_path_excludes_aid_artifacts() {
        assert!(!is_rescuable_path("result-t-abc123.md"));
        assert!(!is_rescuable_path("result-t-0d8f.md"));
        assert!(!is_rescuable_path(".aid/results/foo.md"));
        assert!(is_rescuable_path("results/foo.md"));
        assert!(is_rescuable_path("my-result-t.md"));
    }
}
