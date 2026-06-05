// Task ID claiming for dispatch preparation.
// Exports bounded insert helpers for generated and explicit task IDs.
// Deps: paths, Store, Task/TaskId, run ID-conflict validation.
use anyhow::{Result, anyhow};
use std::path::PathBuf;

use crate::{paths, store::Store, types::{Task, TaskId}};

use super::run_validate::{IdConflict, resolve_id_conflict};

const MAX_GENERATED_ID_INSERT_ATTEMPTS: usize = 8;
const SQLITE_CONSTRAINT_PRIMARYKEY: i32 = 1555;

pub(super) fn insert_task_claiming_id(
    store: &Store,
    task: &mut Task,
    task_id: &mut TaskId,
    log_path: &mut PathBuf,
    explicit_id: bool,
) -> Result<()> {
    if explicit_id {
        return insert_explicit_task_id(store, task, task_id, log_path);
    }
    insert_generated_task_id(store, task, task_id, log_path)
}

fn insert_explicit_task_id(
    store: &Store,
    task: &mut Task,
    task_id: &mut TaskId,
    log_path: &mut PathBuf,
) -> Result<()> {
    match resolve_id_conflict(store, task_id.as_str())? {
        IdConflict::None => store.insert_task(task),
        IdConflict::ReplaceWaiting => store.replace_waiting_task(task),
        IdConflict::Running => {
            anyhow::bail!(
                "Task '{}' is still running. Stop it first: aid stop {}",
                task_id, task_id
            );
        }
        IdConflict::AutoSuffix(new_id) => {
            aid_info!("[aid] ID '{}' already exists, using '{}'", task_id, new_id);
            *task_id = TaskId(new_id);
            *log_path = paths::log_path(task_id.as_str());
            task.id = task_id.clone();
            task.log_path = Some(log_path.to_string_lossy().to_string());
            store.insert_task(task)
        }
    }
}

fn insert_generated_task_id(
    store: &Store,
    task: &mut Task,
    task_id: &mut TaskId,
    log_path: &mut PathBuf,
) -> Result<()> {
    for attempt in 1..=MAX_GENERATED_ID_INSERT_ATTEMPTS {
        task.id = task_id.clone();
        task.log_path = Some(log_path.to_string_lossy().to_string());
        match store.insert_task(task) {
            Ok(()) => return Ok(()),
            Err(err) if is_primary_key_conflict(&err) && attempt < MAX_GENERATED_ID_INSERT_ATTEMPTS => {
                *task_id = TaskId::generate();
                *log_path = paths::log_path(task_id.as_str());
            }
            Err(err) if is_primary_key_conflict(&err) => {
                return Err(anyhow!(
                    "failed to allocate unique task ID after {MAX_GENERATED_ID_INSERT_ATTEMPTS} attempts"
                ));
            }
            Err(err) => return Err(err),
        }
    }
    Err(anyhow!("failed to allocate unique task ID"))
}

fn is_primary_key_conflict(err: &anyhow::Error) -> bool {
    let Some(rusqlite::Error::SqliteFailure(sqlite_err, _)) =
        err.downcast_ref::<rusqlite::Error>() else {
            return false;
        };
    sqlite_err.extended_code == SQLITE_CONSTRAINT_PRIMARYKEY
}
