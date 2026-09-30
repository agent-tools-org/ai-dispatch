// Read-only settlement compares the existing dispatch snapshot without recovery writes.
// Exports settle to dirty-worktree settlement; violations preserve the checkout.
// Deps: worktree snapshot, Store events, and persisted dispatch baseline.

use anyhow::{Context, Result};
use std::{collections::BTreeSet, path::Path};
use crate::{store::Store, types::{EventKind, TaskEvent, TaskId}};
use super::{DirtyWorktreeAction, RunArgs};

pub(in crate::cmd::run) fn capture_baseline(store: &Store, task_id: &TaskId, dir: Option<&str>) -> Result<Option<Vec<String>>> {
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
    Ok(Some(crate::worktree::capture_worktree_snapshot(Path::new(dir))?.read_only_state(Path::new(dir))?))
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
            return Ok(Vec::new());
        }
        let baseline = baseline.context("Dispatch snapshot is unavailable")?;
        let snapshot = crate::worktree::capture_worktree_snapshot(Path::new(dir))?;
        let current = snapshot.read_only_state(Path::new(dir))?;
        changed_paths(dir, [args.result_file.as_deref(), args.output.as_deref()], baseline, &current)
    })();
    let detail = match comparison {
        Ok(paths) if paths.is_empty() => return Ok(DirtyWorktreeAction::Continue),
        Ok(paths) => format!("Read-only violation: changed paths: {}. Files left unchanged for review.", paths.join(", ")),
        Err(err) => format!("Read-only enforcement failed in {dir}: {err:#}. Files left unchanged for review."),
    };
    aid_warn!("[aid] {detail}");
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(), timestamp: chrono::Local::now(),
        event_kind: EventKind::Error, detail, metadata: None,
    })?;
    crate::task_lifecycle::mark_failed(store, task_id)?;
    Ok(DirtyWorktreeAction::Failed)
}

fn changed_paths(
    dir: &str,
    artifacts: [Option<&str>; 2],
    baseline: &[String],
    current: &[String],
) -> Result<Vec<String>> {
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
    let mut paths = BTreeSet::new();
    for entry in before.symmetric_difference(&after) {
        let (path, _, _, _): (String, String, String, u32) = serde_json::from_str(entry)?;
        if artifacts.iter().any(|file| file.components().eq(dir.join(&path).components())) {
            continue;
        }
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}
