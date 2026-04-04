// Diff rendering helpers for `aid show`.
// Exports: diff_text, diff_stat, parse_diff_stat, worktree_diff.
// Deps: cmd::show::load_task, paths, show_output_messages::read_task_output, Store, Task.
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent};

use super::show_output_artifacts::diff_artifact_fallback;

const DIFF_EXCLUDE: &[&str] = &[":(exclude)*.lock", ":(exclude)package-lock.json"];

pub fn diff_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    diff_text_with_filter(store, task_id, None)
}

pub fn diff_text_file(store: &Arc<Store>, task_id: &str, file: &str) -> Result<String> {
    diff_text_with_filter(store, task_id, Some(file))
}

fn diff_text_with_filter(store: &Arc<Store>, task_id: &str, file: Option<&str>) -> Result<String> {
    let task = super::super::load_task(store, task_id)?;
    let mut out = format_diff_header(&task);
    let events = store.get_events(task_id)?;
    if !events.is_empty() {
        out.push_str(&format_recent_events(&events));
    }
    if let Some(ref worktree_path) = task.worktree_path
        && Path::new(worktree_path).exists()
    {
        out.push_str(&format_diff_output(&task, worktree_path, file));
        out.push_str(&format!("\nWorktree: {worktree_path}\n"));
        return Ok(out);
    }
    if let Some(fallback) = diff_artifact_fallback(&task, task_id)? {
        out.push_str(&fallback);
        if task.worktree_branch.is_none() {
            out.push_str("\n[aid] In-place edit — use `git diff` to see working tree changes\n");
        }
        return Ok(out);
    }
    if task.worktree_branch.is_none() {
        let repo = task.repo_path.as_deref().unwrap_or(".");
        let wt_diff = inplace_working_diff(repo, file);
        if !wt_diff.is_empty() {
            out.push_str("\n--- Working Tree Changes (in-place edit) ---\n");
            out.push_str(&wt_diff);
            return Ok(out);
        }
        out.push_str(
            "\n--- Artifacts ---\n  (in-place edit — no uncommitted changes detected, may already be committed)\n",
        );
    } else {
        out.push_str("\n--- Artifacts ---\n  (worktree removed or diff unavailable)\n");
    }
    Ok(out)
}

pub(crate) fn worktree_diff(task: &Task, task_id: &str) -> Result<String> {
    if let Some(ref worktree_path) = task.worktree_path
        && Path::new(worktree_path).exists()
    {
        return Ok(format_diff_output(task, worktree_path, None));
    }
    if let Some(fallback) = diff_artifact_fallback(task, task_id)? {
        return Ok(fallback);
    }
    Ok("\n--- Artifacts ---\n  (no worktree diff or output file available)\n".to_string())
}

fn format_diff_header(task: &Task) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== Review: {} ===\n", task.id));
    out.push_str(&format!(
        "Agent: {}  Status: {}  Prompt: {}\n",
        task.agent_display_name(),
        task.status.label(),
        truncate(&task.prompt, 60),
    ));
    if let Some(ref model) = task.model {
        out.push_str(&format!("Model: {model}\n"));
    }
    out
}

fn format_recent_events(events: &[TaskEvent]) -> String {
    let mut out = String::new();
    out.push_str("\n--- Events (last 10) ---\n");
    let start = events.len().saturating_sub(10);
    for event in &events[start..] {
        let kind = event.event_kind.as_str();
        let time = event.timestamp.format("%H:%M:%S");
        let detail = truncate(&event.detail, 80);
        let marker = if event.event_kind == EventKind::Error {
            "!"
        } else {
            " "
        };
        out.push_str(&format!("{marker} [{time}] {kind}: {detail}\n"));
    }
    out
}

fn format_diff_output(task: &Task, worktree_path: &str, file: Option<&str>) -> String {
    if task.status == crate::types::TaskStatus::Failed
        && task
            .start_sha
            .as_deref()
            .is_some_and(|start_sha| head_matches_start(worktree_path, start_sha))
    {
        return "\n--- Diff Stat ---\nNo changes (task failed before making commits)\n".to_string();
    }
    let mut out = String::new();
    out.push_str("\n--- Diff Stat ---\n");
    let stat = match file {
        Some(path) => diff_stat_file(worktree_path, task.start_sha.as_deref(), path),
        None => diff_stat(worktree_path, task.start_sha.as_deref()),
    };
    out.push_str(&stat);
    out.push_str("\n--- Full Diff ---\n");
    let diff = match file {
        Some(path) => full_diff_file(worktree_path, task.start_sha.as_deref(), path),
        None => full_diff(worktree_path, task.start_sha.as_deref()),
    };
    out.push_str(&diff);
    out
}

fn inplace_working_diff(repo_path: &str, file: Option<&str>) -> String {
    let mut cmd = Command::new("git");
    cmd.args(["-C", repo_path, "diff"]);
    if let Some(file) = file {
        cmd.args(["--", file]);
    } else {
        cmd.args(["--", "."]);
    }
    cmd.args(DIFF_EXCLUDE);
    let output = cmd.output().ok();
    match output {
        Some(o) if o.status.success() && !o.stdout.is_empty() => {
            String::from_utf8_lossy(&o.stdout).into()
        }
        _ => String::new(),
    }
}

pub(crate) fn diff_stat(wt_path: &str, start_sha: Option<&str>) -> String {
    let start_args =
        start_sha.map(|sha| vec!["diff".to_string(), format!("{sha}..HEAD"), "--stat".to_string()]);
    generate_diff(
        wt_path,
        diff_arg_sets(start_args, &[&["diff", "main...HEAD", "--stat"], &["diff", "--stat"], &["diff", "--stat", "HEAD~1"]]).as_slice(),
        "  (no changes detected)\n",
    )
}

pub(crate) fn diff_stat_file(wt_path: &str, start_sha: Option<&str>, file: &str) -> String {
    let start_args =
        start_sha.map(|sha| vec!["diff".to_string(), format!("{sha}..HEAD"), "--stat".to_string()]);
    generate_diff_file(
        wt_path,
        diff_arg_sets(start_args, &[&["diff", "main...HEAD", "--stat"], &["diff", "--stat"], &["diff", "--stat", "HEAD~1"]]).as_slice(),
        "  (no changes detected)\n",
        file,
    )
}

pub(crate) fn parse_diff_stat(diff_text: &str) -> Vec<serde_json::Value> {
    diff_text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || !line.contains('|') {
                return None;
            }
            let mut parts = line.splitn(2, '|');
            let file = parts.next()?.trim();
            let stats = parts.next()?.trim();
            if stats.starts_with("Bin") {
                return None;
            }
            let insertions = stats.chars().filter(|c| *c == '+').count() as u64;
            let deletions = stats.chars().filter(|c| *c == '-').count() as u64;
            if insertions == 0 && deletions == 0 {
                return None;
            }
            Some(json!({
                "file": file,
                "insertions": insertions,
                "deletions": deletions,
            }))
        })
        .collect()
}

fn full_diff(wt_path: &str, start_sha: Option<&str>) -> String {
    let start_args = start_sha.map(|sha| vec!["diff".to_string(), format!("{sha}..HEAD")]);
    generate_diff(
        wt_path,
        diff_arg_sets(start_args, &[&["diff", "main...HEAD"], &["diff"], &["diff", "HEAD~1"]]).as_slice(),
        "  (no diff available)\n",
    )
}

fn full_diff_file(wt_path: &str, start_sha: Option<&str>, file: &str) -> String {
    let start_args = start_sha.map(|sha| vec!["diff".to_string(), format!("{sha}..HEAD")]);
    generate_diff_file(
        wt_path,
        diff_arg_sets(start_args, &[&["diff", "main...HEAD"], &["diff"], &["diff", "HEAD~1"]]).as_slice(),
        "  (no diff available)\n",
        file,
    )
}

fn generate_diff(wt_path: &str, args_sets: &[Vec<String>], fallback: &str) -> String {
    for args in args_sets {
        if let Some(output) = run_git_diff(wt_path, &diff_args(args))
            && !output.trim().is_empty()
        {
            return output;
        }
    }
    fallback.to_string()
}

fn generate_diff_file(wt_path: &str, args_sets: &[Vec<String>], fallback: &str, file: &str) -> String {
    for args in args_sets {
        if let Some(output) = run_git_diff(wt_path, &diff_args_file(args, file))
            && !output.trim().is_empty()
        {
            return output;
        }
    }
    fallback.to_string()
}

fn diff_args(base_args: &[String]) -> Vec<String> {
    let mut args = base_args.to_vec();
    args.push("--".to_string());
    args.push(".".to_string());
    args.extend(DIFF_EXCLUDE.iter().map(|value| value.to_string()));
    args
}

fn diff_args_file(base_args: &[String], file: &str) -> Vec<String> {
    let mut args = base_args.to_vec();
    args.push("--".to_string());
    args.push(file.to_string());
    args
}

fn diff_arg_sets(start_args: Option<Vec<String>>, fallback: &[&[&str]]) -> Vec<Vec<String>> {
    let mut args_sets = Vec::with_capacity(fallback.len() + usize::from(start_args.is_some()));
    if let Some(start_args) = start_args {
        args_sets.push(start_args);
    }
    args_sets.extend(fallback.iter().map(|args| args.iter().map(|value| (*value).to_string()).collect()));
    args_sets
}

fn head_matches_start(wt_path: &str, start_sha: &str) -> bool {
    let output = Command::new("git")
        .args(["-C", wt_path, "rev-parse", "HEAD"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim() == start_sha
        }
        _ => false,
    }
}

fn run_git_diff(wt_path: &str, args: &[String]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(wt_path)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..end])
    }
}
