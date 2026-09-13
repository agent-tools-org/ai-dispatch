// Pluggable backup of terminal task artifacts to off-machine targets.
// Exports: BackupTarget, BackupDest, BackupRef, on_settled, on_terminal, run_backup_with.
// Deps: Store, task types, and the config/bundle/gdrive submodules.

mod bundle;
mod config;
mod gdrive;

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

use std::path::Path;

use anyhow::{anyhow, Result};

use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId, TaskStatus};

pub use config::{BackupGlobalConfig, BackupProjectConfig};
pub(crate) use config::{BackupSettings, Trigger};
pub use gdrive::GdriveTarget;

/// Where a bundle should land inside a target: a `/`-separated folder path
/// (already template-expanded) and the file name to store it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupDest {
    pub folder: String,
    pub file_name: String,
}

/// What a target hands back after a successful upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRef {
    pub id: String,
    pub url: String,
}

pub trait BackupTarget {
    fn name(&self) -> &str;
    fn upload(&self, bundle: &Path, dest: &BackupDest) -> Result<BackupRef>;
}

/// Backs up a task once its post-run lifecycle has settled (final status,
/// verify status and result file persisted). Reads the status from the store.
pub(crate) fn on_settled(store: &Store, task_id: &str) {
    if let Ok(Some(task)) = store.get_task(task_id) {
        on_terminal(store, task_id, task.status);
    }
}

/// The backup hook: called once a task's terminal status is settled, either
/// from `task_lifecycle` for reaper/stop transitions that nothing follows, or
/// from the end of the post-run lifecycle. Runs at most once per task: any
/// earlier attempt, successful or failed, suppresses another. Never returns an
/// error and never changes the task's status; every failure becomes a warning
/// event plus a stderr line.
pub(crate) fn on_terminal(store: &Store, task_id: &str, status: TaskStatus) {
    let Some(trigger) = Trigger::for_status(status) else { return };
    let settings = match config::resolve_for_task(store, task_id) {
        Ok(Some(settings)) => settings,
        Ok(None) => return,
        Err(err) => return warn(store, task_id, &err),
    };
    if !settings.on.contains(&trigger) || already_attempted(store, task_id) {
        return;
    }
    let Some(target) = target_for(&settings) else {
        let err = anyhow!("unknown backup target '{}' (available: gdrive)", settings.target);
        return warn(store, task_id, &err);
    };
    run_backup_with(store, task_id, &settings, target.as_ref());
}

/// True once a URL is recorded or any event carries a `backup` marker, so a
/// failed attempt counts as the task's one attempt.
pub(crate) fn already_attempted(store: &Store, task_id: &str) -> bool {
    if matches!(store.backup_url(task_id), Ok(Some(_))) {
        return true;
    }
    store.get_events(task_id).is_ok_and(|events| {
        events
            .iter()
            .any(|event| event.metadata.as_ref().is_some_and(|meta| meta.get("backup").is_some()))
    })
}

fn target_for(settings: &BackupSettings) -> Option<Box<dyn BackupTarget>> {
    match settings.target.as_str() {
        "gdrive" => Some(Box::new(match &settings.binary {
            Some(binary) => GdriveTarget::with_binary(binary),
            None => GdriveTarget::new(),
        })),
        _ => None,
    }
}

/// Bundle the task's artifacts and upload them through `target`. Returns the
/// recorded URL on success; failures are recorded as warnings and yield `None`.
pub(crate) fn run_backup_with(
    store: &Store,
    task_id: &str,
    settings: &BackupSettings,
    target: &dyn BackupTarget,
) -> Option<String> {
    match perform(store, task_id, settings, target) {
        Ok(backup) => {
            record_success(store, task_id, target.name(), &backup);
            Some(backup.url)
        }
        Err(err) => {
            warn(store, task_id, &err);
            None
        }
    }
}

fn perform(
    store: &Store,
    task_id: &str,
    settings: &BackupSettings,
    target: &dyn BackupTarget,
) -> Result<BackupRef> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("task '{task_id}' not found"))?;
    let bundle = bundle::build(store, &task, &settings.include)?;
    let dest = BackupDest {
        folder: config::expand_folder(&settings.folder, &task),
        file_name: bundle.file_name.clone(),
    };
    target.upload(&bundle.path, &dest)
}

fn record_success(store: &Store, task_id: &str, target: &str, backup: &BackupRef) {
    if let Err(err) = store.set_backup_url(task_id, &backup.url) {
        return warn(store, task_id, &err.context("recording backup url"));
    }
    let _ = store.insert_event(&TaskEvent {
        task_id: TaskId(task_id.to_string()),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: format!("Backup uploaded to {target}: {}", backup.url),
        metadata: Some(serde_json::json!({
            "backup": "uploaded", "backup_url": backup.url, "backup_id": backup.id,
        })),
    });
}

/// A backup failure is a milestone, not an error event: `latest_error` must
/// keep reporting the agent's own failure, and the marker still counts as the
/// task's one attempt.
fn warn(store: &Store, task_id: &str, err: &anyhow::Error) {
    let detail = format!("Backup failed: {err:#}");
    eprintln!("[aid] {task_id}: {detail}");
    let _ = store.insert_event(&TaskEvent {
        task_id: TaskId(task_id.to_string()),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail,
        metadata: Some(serde_json::json!({"backup": "failed"})),
    });
}
