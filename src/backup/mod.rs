// Pluggable backup of terminal task artifacts to off-machine targets.
// Exports: BackupTarget, BackupDest, BackupRef, on_settled, sweep, run_backup_with.
// Deps: Store, task types, and the config/bundle/gdrive/sweep submodules.

mod bundle;
mod config;
mod gdrive;
mod sweep;

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
#[cfg(test)]
#[path = "sweep_tests.rs"]
mod sweep_tests;

use std::path::Path;

use anyhow::{anyhow, Result};

use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};

pub use config::{BackupGlobalConfig, BackupProjectConfig};
pub(crate) use config::{BackupSettings, Trigger};
pub use gdrive::GdriveTarget;
pub(crate) use sweep::sweep;

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

/// Entry point for the post-run lifecycle: backs the task up once its final
/// status, verify status and result file are persisted.
pub(crate) fn on_settled(store: &Store, task_id: &str) {
    attempt(store, task_id);
}

/// Makes the task's one backup attempt if its settled status matches a
/// configured trigger. The once-guard and the atomic claim run before anything
/// can write an event, so a task never produces a second backup milestone of
/// any kind; a resolution error consumes the attempt too. Never returns an
/// error and never changes the task's status. Returns true when the attempt
/// was made (claimed), whatever its outcome.
fn attempt(store: &Store, task_id: &str) -> bool {
    let Ok(Some(task)) = store.get_task(task_id) else { return false };
    let Some(trigger) = Trigger::for_status(task.status) else { return false };
    if already_attempted(store, task_id) {
        return false;
    }
    let settings = match config::resolve_for_task(store, task_id) {
        Ok(Some(settings)) if settings.on.contains(&trigger) => Ok(settings),
        Ok(_) => return false,
        Err(err) => Err(err),
    };
    if !store.claim_backup(task_id).unwrap_or(false) {
        return false;
    }
    let resolved = settings.and_then(|settings| match target_for(&settings) {
        Some(target) => Ok((settings, target)),
        None => Err(anyhow!("unknown backup target '{}' (available: gdrive)", settings.target)),
    });
    match resolved {
        Ok((settings, target)) => {
            run_backup_with(store, task_id, &settings, target.as_ref());
        }
        Err(err) => warn(store, task_id, &err),
    }
    true
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
