// Worktree and branch garbage-collection helpers for doctor and auto-cleanup.
// Exports candidate discovery plus safe branch/worktree cleanup primitives.
// Deps: anyhow, crate::project::ProjectConfig, std::process::Command.

use crate::project::ProjectConfig;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Stdio};

pub(crate) const DEFAULT_BRANCH_PREFIXES: &[&str] = &["feat/", "fix/", "refactor/", "chore/"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorReport {
    pub base_branch: String,
    pub prunable_worktrees: Vec<PrunableWorktree>,
    pub deletable_branches: Vec<DeletableBranch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrunableWorktree {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeletableBranch {
    pub branch: String,
    pub reason: MergeReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MergeReason {
    CherryEmpty,
    LogEmpty,
}

impl MergeReason {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CherryEmpty => "merged (git cherry empty)",
            Self::LogEmpty => "rebased/merged (git log empty)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BranchDeleteOutcome {
    Deleted,
    Missing,
    Kept(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorktreeRemoveOutcome {
    Removed,
    Missing,
}

pub(crate) fn managed_branch_prefixes(project: Option<&ProjectConfig>) -> Vec<String> {
    let mut prefixes = DEFAULT_BRANCH_PREFIXES
        .iter()
        .map(|prefix| (*prefix).to_string())
        .collect::<Vec<_>>();
    if let Some(prefix) = project.and_then(|item| item.worktree_prefix.as_deref()) {
        let prefix = prefix.trim();
        if !prefix.is_empty() && !prefixes.iter().any(|existing| existing == prefix) {
            prefixes.push(prefix.to_string());
        }
    }
    prefixes
}

pub(crate) fn is_managed_branch(branch: &str, prefixes: &[String]) -> bool {
    !is_protected_branch(branch)
        && prefixes.iter().any(|prefix| branch.starts_with(prefix))
}

pub(crate) fn detect_default_branch(repo_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
        ])
        .output()
        .context("Failed to detect default branch")?;
    if !output.status.success() {
        return Ok("main".to_string());
    }
    let reference = String::from_utf8_lossy(&output.stdout);
    let branch = reference
        .trim()
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("main");
    Ok(branch.to_string())
}

pub(crate) fn collect_doctor_report(
    repo_dir: &Path,
    tracked_paths: &BTreeSet<String>,
    prefixes: &[String],
) -> Result<DoctorReport> {
    let base_branch = detect_default_branch(repo_dir)?;
    let prunable_worktrees = prunable_worktrees(repo_dir, tracked_paths)?;
    let deletable_branches = deletable_branches(repo_dir, &base_branch, prefixes)?;
    Ok(DoctorReport {
        base_branch,
        prunable_worktrees,
        deletable_branches,
    })
}

pub(crate) fn branch_merge_reason(
    repo_dir: &Path,
    base_branch: &str,
    branch: &str,
) -> Result<Option<MergeReason>> {
    if branch == base_branch || !local_branch_exists(repo_dir, branch)? {
        return Ok(None);
    }

    let cherry = git_stdout(repo_dir, &["cherry", base_branch, branch])?;
    let log = git_stdout(repo_dir, &["log", "--oneline", &format!("{base_branch}..{branch}")])?;
    Ok(merge_reason_from_outputs(&cherry, &log))
}

pub(crate) fn remove_worktree_path(repo_dir: &Path, worktree_path: &Path) -> Result<WorktreeRemoveOutcome> {
    if !worktree_path.exists() {
        return Ok(WorktreeRemoveOutcome::Missing);
    }
    let status = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "worktree",
            "remove",
            &worktree_path.to_string_lossy(),
        ])
        .status()
        .with_context(|| format!("Failed to remove worktree {}", worktree_path.display()))?;
    if !status.success() {
        anyhow::bail!("git worktree remove failed for {}", worktree_path.display());
    }
    Ok(WorktreeRemoveOutcome::Removed)
}

pub(crate) fn delete_local_branch(repo_dir: &Path, branch: &str) -> Result<BranchDeleteOutcome> {
    if !local_branch_exists(repo_dir, branch)? {
        return Ok(BranchDeleteOutcome::Missing);
    }
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "branch", "-d", branch])
        .output()
        .with_context(|| format!("Failed to delete branch {branch}"))?;
    if output.status.success() {
        return Ok(BranchDeleteOutcome::Deleted);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let note = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("git branch -d refused")
        .trim()
        .to_string();
    Ok(BranchDeleteOutcome::Kept(note))
}

pub(crate) fn tracked_worktree_paths(store: &crate::store::Store) -> Result<BTreeSet<String>> {
    Ok(store
        .list_tasks(crate::types::TaskFilter::All)?
        .into_iter()
        .filter_map(|task| task.worktree_path)
        .collect())
}

fn prunable_worktrees(repo_dir: &Path, tracked_paths: &BTreeSet<String>) -> Result<Vec<PrunableWorktree>> {
    let output = git_stdout(repo_dir, &["worktree", "list", "--porcelain"])?;
    Ok(parse_worktree_entries(&output)
        .into_iter()
        .filter(|entry| entry.prunable && should_consider_worktree(&entry.path, tracked_paths))
        .map(|entry| PrunableWorktree { path: entry.path })
        .collect())
}

fn deletable_branches(
    repo_dir: &Path,
    base_branch: &str,
    prefixes: &[String],
) -> Result<Vec<DeletableBranch>> {
    let branches = git_stdout(repo_dir, &["for-each-ref", "--format=%(refname:short)", "refs/heads"])?;
    let mut deletable = Vec::new();
    for branch in branches.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if !is_managed_branch(branch, prefixes) {
            continue;
        }
        if let Some(reason) = branch_merge_reason(repo_dir, base_branch, branch)? {
            deletable.push(DeletableBranch {
                branch: branch.to_string(),
                reason,
            });
        }
    }
    Ok(deletable)
}

fn local_branch_exists(repo_dir: &Path, branch: &str) -> Result<bool> {
    let status = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("Failed to inspect branch {branch}"))?;
    Ok(status.success())
}

fn git_stdout(repo_dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn is_protected_branch(branch: &str) -> bool {
    matches!(branch, "main" | "master") || branch.starts_with("release/")
}

fn merge_reason_from_outputs(cherry: &str, log: &str) -> Option<MergeReason> {
    if cherry.trim().is_empty() {
        return Some(MergeReason::CherryEmpty);
    }
    if log.trim().is_empty() {
        return Some(MergeReason::LogEmpty);
    }
    None
}

fn should_consider_worktree(path: &str, tracked_paths: &BTreeSet<String>) -> bool {
    path.starts_with("/tmp/aid-wt-")
        || path.starts_with("/private/tmp/aid-wt-")
        || tracked_paths.iter().any(|tracked| same_tmp_worktree_path(tracked, path))
}

fn same_tmp_worktree_path(left: &str, right: &str) -> bool {
    normalize_tmp_path(left) == normalize_tmp_path(right)
}

fn normalize_tmp_path(path: &str) -> &str {
    path.strip_prefix("/private").unwrap_or(path)
}

#[derive(Debug)]
struct WorktreeEntry {
    path: String,
    prunable: bool,
}

fn parse_worktree_entries(output: &str) -> Vec<WorktreeEntry> {
    output
        .split("\n\n")
        .filter_map(|block| {
            let mut path = None;
            let mut prunable = false;
            for line in block.lines().map(str::trim) {
                if let Some(value) = line.strip_prefix("worktree ") {
                    path = Some(value.to_string());
                }
                if line.starts_with("prunable ") {
                    prunable = true;
                }
            }
            path.map(|path| WorktreeEntry { path, prunable })
        })
        .collect()
}

#[cfg(test)]
#[path = "worktree_gc_tests.rs"]
mod tests;
