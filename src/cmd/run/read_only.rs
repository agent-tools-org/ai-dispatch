// Read-only settlement compares the existing dispatch snapshot without recovery writes.
// Exports settle to dirty-worktree settlement; violations preserve the checkout.
// Deps: worktree snapshot, Store events, and persisted dispatch baseline.

use anyhow::{Context, Result};
use std::{collections::BTreeSet, path::Path};
use crate::{store::Store, types::{EventKind, TaskEvent, TaskId}};
use super::{DirtyWorktreeAction, RunArgs};

pub(super) fn settle(
    store: &Store,
    task_id: &TaskId,
    args: &RunArgs,
    dir: &str,
    baseline: Option<&[String]>,
) -> Result<DirtyWorktreeAction> {
    let comparison = (|| {
        let baseline = baseline.context("Dispatch snapshot is unavailable")?;
        let snapshot = crate::worktree::capture_worktree_snapshot(Path::new(dir))?;
        let current = snapshot.read_only_state(Path::new(dir))?;
        changed_paths(dir, args.result_file.as_deref(), baseline, &current)
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
    result_file: Option<&str>,
    baseline: &[String],
    current: &[String],
) -> Result<Vec<String>> {
    let before: BTreeSet<_> = baseline.iter().collect();
    let after: BTreeSet<_> = current.iter().collect();
    let result = result_file.map(|file| Path::new(dir).join(file));
    let mut paths = BTreeSet::new();
    for entry in before.symmetric_difference(&after) {
        let (path, _, _, _): (String, String, String, u32) = serde_json::from_str(entry)?;
        if result.as_ref().is_some_and(|file| file.components().eq(Path::new(dir).join(&path).components())) {
            continue;
        }
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}
