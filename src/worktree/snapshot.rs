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
    /// Whether anything the *agent* left is uncommitted.
    ///
    /// aid's own bookkeeping does not count. It used to: a task that committed
    /// cleanly still failed because aid removed its own `.aid-lock` on the way out,
    /// `git status` reported ` D .aid-lock`, and the caller read that as agent dirt —
    /// dispatching a retry and skipping the configured verification. Rescue already
    /// ignored the file (`is_rescuable_path`), so rescue staged nothing while the
    /// gate downstream acted as if it had; the two policies have to agree.
    pub fn has_uncommitted_changes(&self) -> bool {
        !self.agent_status_lines().is_empty()
    }

    /// Status lines with aid's own files removed — what a human should be shown when
    /// asked "what did the agent leave behind".
    pub fn agent_status_lines(&self) -> Vec<String> {
        self.status_lines
            .iter()
            .filter(|line| !line_is_only_aid_owned(line))
            .cloned()
            .collect()
    }

    pub fn rescuable_entries(&self) -> Vec<WorktreeStatusEntry> {
        self.entries
            .iter()
            .filter(|entry| is_rescuable_path(&entry.path))
            .cloned()
            .collect()
    }
}

/// Whether a porcelain line names nothing but aid's own files.
///
/// A rename names two paths and `extract_baseline_path` returns only the destination,
/// so judging on that alone would drop `R  src/lib.rs -> result-t-abcd.md` entirely and
/// hide a real file's disappearance from every dirty check. Both sides have to be aid's
/// before the line stops counting.
fn line_is_only_aid_owned(line: &str) -> bool {
    let paths = status_line_paths(line);
    !paths.is_empty() && paths.iter().all(|path| is_aid_owned_path(path))
}

/// Every path a porcelain line names: one, or two for a rename.
fn status_line_paths(line: &str) -> Vec<String> {
    let Some(rest) = line.strip_prefix("?? ").or_else(|| line.get(3..)) else {
        return Vec::new();
    };
    match rest.split_once(" -> ") {
        Some((from, to)) => vec![unquote(from), unquote(to)],
        None => vec![unquote(rest)],
    }
}

/// Git quotes paths that need escaping (`?? "odd name.md"`). Strip the wrapper so the
/// name is judged, not the quote character.
fn unquote(path: &str) -> String {
    path.strip_prefix('"').and_then(|p| p.strip_suffix('"')).unwrap_or(path).to_string()
}

/// Paths aid itself writes into a worktree. Deliberately narrower than
/// `is_rescuable_path`, which also rejects build output: a stray `target/` or `.so`
/// is still the agent's business and must keep failing the data-loss assertion.
///
/// It is also narrower than `AID_ADD_EXCLUDES`, which still matches any `result-*.md`.
/// That breadth is safe for `git add` — over-excluding there only leaves a file
/// uncommitted — but on this path an over-match makes real work stop counting as
/// uncommitted, so the two lists are deliberately not the same.
pub fn is_aid_owned_path(path: &str) -> bool {
    // Porcelain reports an untracked directory with a trailing slash; keep the last
    // real segment so `.aid-dir/` is judged on its name and not on an empty string.
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    name.starts_with(".aid-")
        || name.starts_with("aid-batch-")
        || path == ".aid/state.toml"
        || path.starts_with(".aid/batches/")
        // `result-t-`, never a bare `result-`: aid's report is always named for its
        // task id, and widening this to any `result-*.md` would quietly classify a
        // user's own `result-summary.md` as aid's — on this path an over-match means
        // real work stops counting as uncommitted and can be thrown away.
        || (name.starts_with("result-t-") && (name.ends_with(".md") || name.ends_with(".json")))
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
    capture_worktree_snapshot_with_base(dir, None)
}

pub fn capture_worktree_snapshot_with_base(
    dir: &Path,
    base_branch: Option<&str>,
) -> Result<WorktreeSnapshot> {
    let status_lines = read_status_lines(dir)?;
    let entries = status_lines
        .iter()
        .filter_map(|line| parse_status_entry(line))
        .collect();
    Ok(WorktreeSnapshot {
        status_lines,
        entries,
        empty_diff: read_empty_diff(dir, base_branch),
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

/// `git add` pathspec exclusions for aid's own runtime bookkeeping/artifacts —
/// a target repo won't gitignore these itself, so any `git add` run by aid or
/// a dispatched agent must exclude them explicitly.
///
/// This is a *different* policy from `is_rescuable_path`, not the same one
/// restated: `is_rescuable_path` decides what rescue may treat as recoverable
/// source, and can afford to reject all of `.aid/` conservatively since it
/// only ever considers new/dirty files. This list feeds `git add -u` as well
/// (see `commit::auto_commit`), which restages already-tracked files — an
/// unqualified `.aid/**` exclude here would stop a repo that legitimately
/// tracks `.aid/project.toml` from ever having edits to it committed. So this
/// list names only the paths aid itself generates under `.aid/`
/// (`.aid/state.toml`, `.aid/batches/`, per `.gitignore`), not the directory
/// as a whole.
///
/// Each `.aid-*`/`aid-batch-*` pattern is listed twice (bare and `**/`-
/// prefixed): git pathspec matching does not let a single glob cover both a
/// repo-root file and a nested one of the same name.
pub const AID_ADD_EXCLUDES: &[&str] = &[
    ":(exclude).aid-*",
    ":(exclude)**/.aid-*",
    ":(exclude).aid/state.toml",
    ":(exclude).aid/batches/**",
    ":(exclude)result-*.md",
    ":(exclude)result-*.json",
    ":(exclude)aid-batch-*",
    ":(exclude)**/aid-batch-*",
];

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

fn read_empty_diff(dir: &Path, base_branch: Option<&str>) -> Option<bool> {
    let head = git_diff_stat_output(dir, &["diff", "--stat", "HEAD"])?;
    let staged = git_diff_stat_output(dir, &["diff", "--cached", "--stat"])?;
    let committed = read_committed_diff_empty(dir, base_branch).unwrap_or(true);
    Some(head.trim().is_empty() && staged.trim().is_empty() && committed)
}

fn read_committed_diff_empty(dir: &Path, base_branch: Option<&str>) -> Option<bool> {
    let base = base_branch
        .filter(|branch| git_ref_exists(dir, branch))
        .map(str::to_string)
        .or_else(|| detect_default_branch(dir))?;
    let range = format!("{base}...HEAD");
    let diff = git_diff_stat_output(dir, &["diff", "--stat", &range])?;
    Some(diff.trim().is_empty())
}

fn detect_default_branch(dir: &Path) -> Option<String> {
    git_output_line(dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .or_else(|| git_ref_name_if_exists(dir, "main"))
        .or_else(|| git_ref_name_if_exists(dir, "master"))
}

fn git_ref_name_if_exists(dir: &Path, name: &str) -> Option<String> {
    git_ref_exists(dir, name).then(|| name.to_string())
}

fn git_ref_exists(dir: &Path, name: &str) -> bool {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{name}^{{commit}}"))
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn git_output_line(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::to_string)
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
#[path = "snapshot_tests.rs"]
mod tests;
