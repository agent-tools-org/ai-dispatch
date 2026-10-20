// Read-only settlement compares the existing dispatch snapshot without recovery writes.
// Exports settle to dirty-worktree settlement; violations preserve the checkout.
// Deps: worktree snapshot, Store events, and persisted dispatch baseline.

use anyhow::{Context, Result};
use std::{collections::BTreeSet, path::Path};
use crate::{store::Store, types::{EventKind, TaskEvent, TaskId}};
use super::{DirtyWorktreeAction, RunArgs};

pub(in crate::cmd::run) fn capture_baseline(store: &Store, task_id: &TaskId, dir: Option<&str>) -> Result<Option<Vec<String>>> {
    crate::runtime::read_only_launch::reset(store, task_id)?;
    let dir = dir.context("Read-only run directory is unavailable")?;
    if !is_git_directory(dir)? {
        let detail = format!("Warning: read-only enforcement unavailable in {dir}: not a git repository. Enforcement covers Git run directories only.");
        aid_warn!("[aid] {detail}");
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(), timestamp: chrono::Local::now(),
            event_kind: EventKind::Setup, detail, metadata: Some(serde_json::json!({"warning": true})),
        })?;
        return Ok(None);
    }
    Ok(Some(crate::workspace::worktree::capture_worktree_snapshot(Path::new(dir))?.read_only_state(Path::new(dir))?))
}

fn is_git_directory(dir: &str) -> Result<bool> {
    let output = std::process::Command::new("git").current_dir(dir).env("LC_ALL", "C")
        .args(["rev-parse", "--show-toplevel"]).output().context("Failed to resolve Git run directory")?;
    if output.status.success() { return Ok(true); }
    let reason = String::from_utf8_lossy(&output.stderr);
    if reason.starts_with("fatal: not a git repository (or any of the parent directories)") { return Ok(false); }
    anyhow::bail!("Git run directory resolution failed: {reason}")
}

pub(super) fn settle(
    store: &Store,
    task_id: &TaskId,
    args: &RunArgs,
    dir: &str,
    baseline: Option<&[String]>,
) -> Result<DirtyWorktreeAction> {
    let comparison = (|| {
        if baseline.is_none() && !is_git_directory(dir)? {
            return Ok((Vec::new(), Vec::new()));
        }
        let baseline = baseline.context("Dispatch snapshot is unavailable")?;
        let snapshot = crate::workspace::worktree::capture_worktree_snapshot(Path::new(dir))?;
        let current = snapshot.read_only_state(Path::new(dir))?;
        let (paths, refs) = changed_paths(
            dir, [args.result_file.as_deref(), args.output.as_deref()], baseline, &current,
        )?;
        if !paths.is_empty() && crate::runtime::read_only_launch::enforced(store, task_id, dir)? {
            record_concurrent_change(store, task_id, baseline, &current, &paths)?;
            return Ok((Vec::new(), refs));
        }
        Ok((paths, refs))
    })();
    let detail = match comparison {
        Ok((paths, refs)) => {
            record_ref_diagnostic(store, task_id, &refs)?;
            if paths.is_empty() { return Ok(DirtyWorktreeAction::Continue); }
            format!("Read-only violation: changed paths: {}. Files left unchanged for review.", paths.join(", "))
        }
        Err(err) => format!("Read-only enforcement failed in {dir}: {err:#}. Files left unchanged for review."),
    };
    aid_warn!("[aid] {detail}");
    store.insert_agent_event(&TaskEvent {
        task_id: task_id.clone(), timestamp: chrono::Local::now(),
        event_kind: EventKind::Error, detail, metadata: None,
    })?;
    crate::task::lifecycle::mark_failed(store, task_id)?;
    Ok(DirtyWorktreeAction::Failed)
}

fn record_concurrent_change(
    store: &Store, task: &TaskId, baseline: &[String], current: &[String], paths: &[String],
) -> Result<()> {
    let baseline_head = snapshot_head(baseline)?;
    let current_head = snapshot_head(current)?;
    let detail = format!(
        "Warning: audited inputs changed during the run (concurrent checkout change); \
         input stability is unknown. Baseline HEAD: {baseline_head}; current HEAD: {current_head}; \
         changed paths: {}.", paths.join(", "),
    );
    aid_warn!("[aid] {detail}");
    store.insert_event(&TaskEvent {
        task_id: task.clone(), timestamp: chrono::Local::now(), event_kind: EventKind::Setup,
        detail, metadata: Some(serde_json::json!({
            "warning": "read_only_concurrent_change", "input_stability": "unknown",
            "baseline_head": baseline_head, "current_head": current_head, "changed_paths": paths,
        })),
    })
}

fn snapshot_head(snapshot: &[String]) -> Result<String> {
    for entry in snapshot {
        let (path, _, content, _): (String, String, String, u32) = serde_json::from_str(entry)?;
        if path == ".git/HEAD" {
            return Ok(content.rsplit_once(':').context("Malformed snapshot HEAD")?.1.to_string());
        }
    }
    anyhow::bail!("Snapshot HEAD is unavailable")
}

fn record_ref_diagnostic(store: &Store, task_id: &TaskId, refs: &[String]) -> Result<()> {
    if !refs.is_empty() {
        store.insert_event(&TaskEvent {
            task_id: task_id.clone(), timestamp: chrono::Local::now(), event_kind: EventKind::Setup,
            detail: format!("Read-only diagnostic: unrelated refs changed: {}", refs.join(", ")),
            metadata: Some(serde_json::json!({"diagnostic": "unrelated_ref_changes", "refs": refs})),
        })?;
    }
    Ok(())
}

fn changed_paths(
    dir: &str,
    artifacts: [Option<&str>; 2],
    baseline: &[String],
    current: &[String],
) -> Result<(Vec<String>, Vec<String>)> {
    let before: BTreeSet<_> = baseline.iter().collect();
    let after: BTreeSet<_> = current.iter().collect();
    let dir = Path::new(dir).canonicalize()?;
    let artifacts: Vec<_> = artifacts.into_iter().flatten().map(|file| {
        let file = dir.join(file);
        match (file.parent().and_then(|parent| parent.canonicalize().ok()), file.file_name()) {
            (Some(parent), Some(name)) => parent.join(name),
            _ => file,
        }
    }).collect();
    let root = std::process::Command::new("git").current_dir(&dir)
        .args(["rev-parse", "--show-toplevel"]).output()?;
    anyhow::ensure!(root.status.success(), "Failed to resolve read-only repository root");
    let root = String::from_utf8(root.stdout)?;
    let root = Path::new(root.trim_end_matches('\n')).canonicalize()?;
    let branches = head_branches(baseline, current)?;
    let mut paths = BTreeSet::new();
    let mut refs = BTreeSet::new();
    for entry in before.symmetric_difference(&after) {
        let (path, _, _, _): (String, String, String, u32) = serde_json::from_str(entry)?;
        if !path.starts_with(".git/")
            && artifacts.iter().any(|file| file.components().eq(root.join(&path).components())) {
            continue;
        }
        if path.starts_with(".git/refs/") && !branches.contains(&path) {
            refs.insert(path);
            continue;
        }
        paths.insert(path);
    }
    Ok((paths.into_iter().collect(), refs.into_iter().collect()))
}

fn head_branches(baseline: &[String], current: &[String]) -> Result<BTreeSet<String>> {
    let mut branches = BTreeSet::new();
    for entry in baseline.iter().chain(current) {
        let (path, _, content, _): (String, String, String, u32) = serde_json::from_str(entry)?;
        if path == ".git/HEAD" && let Some((branch, _)) = content.split_once(':')
            && branch.starts_with("refs/heads/") {
            branches.insert(format!(".git/{branch}"));
        }
    }
    Ok(branches)
}

#[cfg(test)]
#[path = "read_only_ref_tests.rs"]
mod tests;
