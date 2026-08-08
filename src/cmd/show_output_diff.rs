// Diff rendering helpers for `aid show`.
// Exports: diff_text, diff_text_branch, diff_stat, parse_diff_stat, worktree_diff.
// Deps: show_output_diff_base git plumbing, cmd::show::load_task, Store, Task.
use anyhow::Result;
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent};
use crate::worktree::{capture_live_worktree_state, uncommitted_diff_text};

use super::show_output_artifacts::diff_artifact_fallback;
use super::show_output_diff_base::{
    branch_diff_signal, generate_diff, generate_diff_file, head_matches_start, merge_base,
    range_arg_sets,
};
use super::worktree_state_section;
pub(crate) use super::show_output_diff_base::branch_base_ref;

const DIFF_EXCLUDE: &[&str] = &[":(exclude)*.lock", ":(exclude)package-lock.json"];

pub fn diff_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    diff_text_with_filter(store, task_id, None, false)
}

/// Everything on the task's branch since it left the default branch, not just
/// the commits this task added. Backs `aid show <id> --diff --branch`.
pub fn diff_text_branch(store: &Arc<Store>, task_id: &str) -> Result<String> {
    diff_text_with_filter(store, task_id, None, true)
}

pub fn diff_text_file(store: &Arc<Store>, task_id: &str, file: &str, branch: bool) -> Result<String> {
    diff_text_with_filter(store, task_id, Some(file), branch)
}

fn diff_text_with_filter(
    store: &Arc<Store>,
    task_id: &str,
    file: Option<&str>,
    branch: bool,
) -> Result<String> {
    let task = super::super::load_task(store, task_id)?;
    let mut out = format_diff_header(&task);
    let events = store.get_events(task_id)?;
    if !events.is_empty() {
        out.push_str(&format_recent_events(&events));
    }
    if let Some(ref worktree_path) = task.worktree_path
        && Path::new(worktree_path).exists()
    {
        out.push_str(&format_diff_output(&task, worktree_path, file, branch));
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
        return Ok(format_diff_output(task, worktree_path, None, false));
    }
    if let Some(fallback) = diff_artifact_fallback(task, task_id)? {
        return Ok(fallback);
    }
    Ok("\n--- Artifacts ---\n  (no worktree diff or output file available)\n".to_string())
}

fn format_diff_header(task: &Task) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== Review: {} ===\n", task.id));
    // Route already carries model + attribution; keep no separate Model line.
    out.push_str(&format!(
        "Route: {}  Status: {}  Prompt: {}\n",
        task.display_route(),
        task.status.label(),
        truncate(&task.prompt, 60),
    ));
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
        let marker = if event.event_kind == EventKind::Error { "!" } else { " " };
        out.push_str(&format!("{marker} [{time}] {kind}: {detail}\n"));
    }
    out
}

fn format_diff_output(task: &Task, worktree_path: &str, file: Option<&str>, branch: bool) -> String {
    let mut out = String::new();
    let live_state = capture_live_worktree_state(Path::new(worktree_path)).ok();
    let failed_without_new_commits = task.status == crate::types::TaskStatus::Failed
        && task.start_sha.as_deref().is_some_and(|start_sha| head_matches_start(worktree_path, start_sha));
    out.push_str(&worktree_state_section(worktree_path, live_state.as_ref()));
    let base_ref = branch_base_ref(worktree_path);
    // --branch rebases the whole view on where this branch left the default branch, so
    // the diff covers every commit on it rather than only this task's own. If no base
    // ref resolves there is no branch to show; say so instead of labelling whatever the
    // fallbacks turn up as "whole branch".
    let branch_base = branch.then(|| base_ref.as_deref().and_then(|base| merge_base(worktree_path, base)));
    let branch_unresolved = branch && branch_base.as_ref().is_none_or(Option::is_none);
    let scoped = branch && !branch_unresolved;
    // flatten, not match: branch mode with an unresolvable base is Some(None), and that
    // has to fall through to the task's own baseline — the message below promises it.
    let baseline = branch_base.clone().flatten().or_else(|| task.start_sha.clone());
    if branch_unresolved {
        out.push_str("\n[aid] Cannot locate this branch's base: no default branch ref resolves in this worktree.\n        Showing this task's own changes instead — the branch view is unavailable, not empty.\n");
    }
    out.push_str(if scoped { "\n--- Diff Stat (whole branch) ---\n" } else { "\n--- Diff Stat ---\n" });
    let stat = match file {
        Some(path) => diff_stat_file(worktree_path, baseline.as_deref(), path, base_ref.as_deref()),
        None => diff_stat(worktree_path, baseline.as_deref(), base_ref.as_deref()),
    };
    if live_state.as_ref().is_some_and(|state| state.is_dirty())
        && stat.contains("(no changes detected)")
    {
        out.push_str("  (no committed diff detected)\n");
    } else if failed_without_new_commits && !scoped {
        out.push_str("No changes (task failed before making commits)\n");
    } else {
        out.push_str(&stat);
    }
    out.push_str(if scoped { "\n--- Full Diff (whole branch) ---\n" } else { "\n--- Full Diff ---\n" });
    let diff = if failed_without_new_commits && file.is_none() && !scoped {
        uncommitted_diff_text(Path::new(worktree_path)).unwrap_or_default()
    } else {
        match file { Some(path) => full_diff_file(worktree_path, baseline.as_deref(), path, base_ref.as_deref()), None => full_diff(worktree_path, baseline.as_deref(), base_ref.as_deref()) }
    };
    out.push_str(if diff.trim().is_empty() { "  (no diff available)\n" } else { &diff });
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

pub(crate) fn diff_stat(wt_path: &str, start_sha: Option<&str>, base_ref: Option<&str>) -> String {
    let mut out = generate_diff(
        wt_path,
        &range_arg_sets(start_sha, base_ref, true),
        "  (no changes detected)\n",
    );
    append_branch_signal(&mut out, wt_path, start_sha, base_ref);
    out
}

pub(crate) fn diff_stat_file(wt_path: &str, start_sha: Option<&str>, file: &str, base_ref: Option<&str>) -> String {
    let mut out = generate_diff_file(
        wt_path,
        &range_arg_sets(start_sha, base_ref, true),
        "  (no changes detected)\n",
        file,
    );
    append_branch_signal(&mut out, wt_path, start_sha, base_ref);
    out
}

fn append_branch_signal(out: &mut String, wt_path: &str, start_sha: Option<&str>, base_ref: Option<&str>) {
    let Some(signal) = branch_diff_signal(wt_path, start_sha, base_ref) else {
        return;
    };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&signal);
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

fn full_diff(wt_path: &str, start_sha: Option<&str>, base_ref: Option<&str>) -> String {
    generate_diff(wt_path, &range_arg_sets(start_sha, base_ref, false), "  (no diff available)\n")
}

fn full_diff_file(wt_path: &str, start_sha: Option<&str>, file: &str, base_ref: Option<&str>) -> String {
    generate_diff_file(
        wt_path,
        &range_arg_sets(start_sha, base_ref, false),
        "  (no diff available)\n",
        file,
    )
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s.floor_char_boundary(max.saturating_sub(3));
        format!("{}...", &s[..end])
    }
}

