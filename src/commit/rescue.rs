// Dirty worktree rescue for agent commits.
// Exports rescue outcome types plus untracked/modified staging helpers.
// Deps: git CLI via std::process and parent commit helpers.

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::worktree::{WorktreeStatusEntry, WorktreeStatusKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescueOutcome {
    pub staged: Vec<String>,
    pub committed: bool,
    pub had_existing_head: bool,
    pub error: Option<String>,
    pub untracked: Vec<String>,
    pub modified: Vec<String>,
}

pub fn detect_untracked_source_files(dir: &str) -> Result<Vec<String>> {
    Ok(detect_rescuable_files(dir, None)?
        .into_iter()
        .filter(|file| file.kind == WorktreeStatusKind::Untracked)
        .map(|file| file.path)
        .collect())
}

pub fn rescue_dirty_worktree(dir: &str, task_id: &str) -> Result<RescueOutcome> {
    rescue_dirty_worktree_with_baseline(dir, task_id, None)
}

pub fn rescue_dirty_worktree_with_baseline(
    dir: &str,
    task_id: &str,
    baseline: Option<&[String]>,
) -> Result<RescueOutcome> {
    let had_existing_head = crate::commit::head_sha(dir).is_ok();
    let files = detect_rescuable_files(dir, baseline)?;
    let mut outcome = RescueOutcome {
        staged: Vec::new(),
        committed: false,
        had_existing_head,
        error: None,
        untracked: Vec::new(),
        modified: Vec::new(),
    };
    for file in files {
        if let Err(err) = stage_file(dir, &file) {
            outcome.error = Some(err);
            return Ok(outcome);
        }
        outcome.staged.push(file.path.clone());
        match file.kind {
            WorktreeStatusKind::Untracked => outcome.untracked.push(file.path),
            WorktreeStatusKind::Modified => outcome.modified.push(file.path),
        }
    }
    if outcome.staged.is_empty() {
        return Ok(outcome);
    }
    if let Err(err) = commit_rescue(dir, task_id, had_existing_head) {
        outcome.error = Some(err);
        return Ok(outcome);
    }
    outcome.committed = true;
    Ok(outcome)
}

#[allow(dead_code)]
pub fn rescue_untracked_files(dir: &str, task_id: &str) -> Result<Vec<String>> {
    Ok(rescue_dirty_worktree(dir, task_id)?.untracked)
}

pub(super) fn stage_untracked_source_files(dir: &str, task_id: &str) -> Result<Vec<String>> {
    let mut staged = Vec::new();
    let files = match detect_untracked_source_files(dir) {
        Ok(files) => files,
        Err(err) => {
            aid_warn!("[aid] Warning: failed to detect untracked files for {task_id}: {err}");
            return Ok(Vec::new());
        }
    };
    for file in files {
        let add = match Command::new("git").args(["-C", dir, "add", "--"]).arg(&file).output() {
            Ok(add) => add,
            Err(err) => {
                aid_warn!("[aid] Warning: failed to stage rescued file for {task_id}: {err}");
                break;
            }
        };
        if !add.status.success() {
            aid_warn!("[aid] Warning: failed to stage rescued file for {task_id}: {}", first_stderr_line(&add.stderr));
            break;
        }
        staged.push(file);
    }
    Ok(staged)
}

fn detect_rescuable_files(
    dir: &str,
    baseline: Option<&[String]>,
) -> Result<Vec<WorktreeStatusEntry>> {
    let baseline_paths = baseline
        .map(extract_baseline_paths)
        .unwrap_or_default();
    Ok(crate::worktree::capture_worktree_snapshot(Path::new(dir))?
        .rescuable_entries()
        .into_iter()
        .filter(|entry| !baseline_contains(&baseline_paths, entry))
        .collect())
}

fn extract_baseline_paths(baseline: &[String]) -> HashSet<String> {
    baseline
        .iter()
        .filter_map(|line| extract_baseline_path(line))
        .collect()
}

fn extract_baseline_path(line: &str) -> Option<String> {
    if line.is_empty() || line.len() < 4 {
        return None;
    }
    if let Some(path) = line.strip_prefix("?? ") {
        return Some(path.to_string());
    }
    let path = &line[3..];
    if let Some((_, renamed_path)) = path.split_once(" -> ") {
        return Some(renamed_path.to_string());
    }
    Some(path.to_string())
}

fn baseline_contains(baseline: &HashSet<String>, entry: &WorktreeStatusEntry) -> bool {
    baseline.contains(&entry.path)
}

fn stage_file(dir: &str, file: &WorktreeStatusEntry) -> std::result::Result<(), String> {
    let mut add = Command::new("git");
    add.args(["-C", dir]);
    match file.kind {
        WorktreeStatusKind::Untracked => {
            add.args(["add", "--"]);
        }
        WorktreeStatusKind::Modified => {
            add.args(["add", "-u", "--"]);
        }
    }
    let output = add.arg(&file.path).output().map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(first_stderr_line(&output.stderr))
    }
}

fn commit_rescue(dir: &str, task_id: &str, had_existing_head: bool) -> std::result::Result<(), String> {
    let output = if had_existing_head && !head_is_tagged(dir)? {
        Command::new("git")
            .args(["-C", dir, "commit", "--amend", "--no-edit"])
            .output()
    } else {
        Command::new("git")
            .args(["-C", dir, "commit", "-m", &format!("[aid] rescue: stage files missed by agent (task: {task_id})")])
            .output()
    }
    .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(first_stderr_line(&output.stderr))
    }
}

fn head_is_tagged(dir: &str) -> std::result::Result<bool, String> {
    let output = Command::new("git")
        .args(["-C", dir, "tag", "--points-at", "HEAD"])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(first_stderr_line(&output.stderr));
    }
    let tagged = !String::from_utf8_lossy(&output.stdout).trim().is_empty();
    if tagged {
        aid_warn!("[aid] rescue: skipped amend because HEAD is tagged; creating a new rescue commit instead");
    }
    Ok(tagged)
}

fn first_stderr_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr).lines().next().unwrap_or("").to_string()
}

#[cfg(test)]
#[path = "rescue_tests.rs"]
mod tests;
