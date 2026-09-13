// Pluggable backup of terminal task artifacts to off-machine targets.
// Exports: BackupTarget, BackupDest, BackupRef, on_terminal, run_backup_with.
// Deps: Store, task types, and the config/bundle/gdrive submodules.

mod bundle;
mod config;
mod gdrive;

#[cfg(test)]
mod tests;

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

/// The single lifecycle hook: called by `task_lifecycle` after a task has
/// changed to a terminal status. Never returns an error and never changes
/// the task's status; every failure becomes a warning event plus a stderr line.
pub(crate) fn on_terminal(store: &Store, task_id: &str, status: TaskStatus) {
    let Some(trigger) = Trigger::for_status(status) else { return };
    let settings = match config::resolve_for_task(store, task_id) {
        Ok(Some(settings)) => settings,
        Ok(None) => return,
        Err(err) => return warn(store, task_id, &err),
    };
    if !settings.on.contains(&trigger) {
        return;
    }
    if matches!(store.backup_url(task_id), Ok(Some(_))) {
        return;
    }
    let Some(target) = target_for(&settings.target) else {
        let err = anyhow!("unknown backup target '{}' (available: gdrive)", settings.target);
        return warn(store, task_id, &err);
    };
    run_backup_with(store, task_id, &settings, target.as_ref());
}

fn target_for(name: &str) -> Option<Box<dyn BackupTarget>> {
    match name {
        "gdrive" => Some(Box::new(GdriveTarget::new())),
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
        metadata: Some(serde_json::json!({"backup_url": backup.url, "backup_id": backup.id})),
    });
}

fn warn(store: &Store, task_id: &str, err: &anyhow::Error) {
    let detail = format!("Backup failed: {err:#}");
    eprintln!("[aid] {task_id}: {detail}");
    let _ = store.insert_event(&TaskEvent {
        task_id: TaskId(task_id.to_string()),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata: Some(serde_json::json!({"backup": "failed"})),
    });
}
