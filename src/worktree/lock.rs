// Worktree lease lock helpers for task dispatch.
// Exports lock acquisition, ownership re-keying, and cleanup checks.
// Deps: Store task status fallback, state process liveness, std fs primitives.

use crate::store::Store;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const LOCK_FILENAME: &str = ".aid-lock";
static LOCK_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
struct LockRecord {
    task_id: String,
    owner_pid: Option<u32>,
    worker_pid: Option<u32>,
    generation: u64,
}

pub fn check_worktree_lock(wt_path: &Path) -> Option<String> {
    check_worktree_lock_with_store(wt_path, None)
}

pub fn check_worktree_lock_with_store(wt_path: &Path, store: Option<&Store>) -> Option<String> {
    let lock_path = wt_path.join(LOCK_FILENAME);
    let record = read_lock_record(&lock_path)?;
    if let Some(pid) = record.worker_pid.or(record.owner_pid) {
        if super::state::process_alive(pid) {
            return Some(record.task_id);
        }
        let _ = std::fs::remove_file(&lock_path);
        return None;
    }
    if task_status_keeps_lock(store, &record.task_id) {
        return Some(record.task_id);
    }
    let _ = std::fs::remove_file(&lock_path);
    None
}

pub fn write_worktree_lock(wt_path: &Path, task_id: &str) {
    let record = LockRecord::new(task_id);
    let _ = write_lock_record(&wt_path.join(LOCK_FILENAME), &record);
}

pub fn rekey_worktree_lock_to_worker(
    wt_path: &Path,
    task_id: &str,
    worker_pid: u32,
) -> std::result::Result<(), String> {
    let lock_path = wt_path.join(LOCK_FILENAME);
    let mut record = read_lock_record(&lock_path).ok_or_else(|| "missing lock".to_string())?;
    if record.task_id != task_id {
        return Err(record.task_id);
    }
    record.worker_pid = Some(worker_pid);
    record.generation = record.generation.saturating_add(1);
    write_lock_record(&lock_path, &record).map_err(|err| err.to_string())
}

pub fn try_acquire_worktree_lock(wt_path: &Path, task_id: &str) -> std::result::Result<(), String> {
    try_acquire_worktree_lock_with_store(wt_path, task_id, None)
}

pub fn try_acquire_worktree_lock_with_store(
    wt_path: &Path,
    task_id: &str,
    store: Option<&Store>,
) -> std::result::Result<(), String> {
    try_acquire_worktree_lock_with_hook(wt_path, task_id, store, || {})
}

fn try_acquire_worktree_lock_with_hook<F>(
    wt_path: &Path,
    task_id: &str,
    store: Option<&Store>,
    after_stale_rename: F,
) -> std::result::Result<(), String>
where
    F: FnOnce(),
{
    let lock_path = wt_path.join(LOCK_FILENAME);
    let mut after_stale_rename = Some(after_stale_rename);
    for attempt in 0..2 {
        match create_lock_file(&lock_path, task_id) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = check_worktree_lock_with_store(wt_path, store);
                if !lock_path.exists() && attempt == 0 {
                    continue;
                }
                if holder.is_none() && attempt == 0 {
                    if let Some(hook) = after_stale_rename.take() {
                        remove_stale_lock(&lock_path, hook)?;
                    } else {
                        remove_stale_lock(&lock_path, || {})?;
                    }
                    continue;
                }
                return Err(holder.unwrap_or_else(|| "unknown".to_string()));
            }
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("unknown".to_string())
}

pub fn clear_worktree_lock(wt_path: &Path, expected_task_id: &str) -> std::result::Result<(), String> {
    let lock_path = wt_path.join(LOCK_FILENAME);
    if let Some(record) = read_lock_record(&lock_path)
        && record.task_id != expected_task_id {
            return Err(record.task_id);
        }
    let _ = std::fs::remove_file(&lock_path);
    sweep_orphan_lock_files(wt_path);
    Ok(())
}

fn create_lock_file(lock_path: &Path, task_id: &str) -> std::io::Result<()> {
    let temp_path = lock_path.with_extension(unique_lock_extension("tmp"));
    let record = LockRecord::new(task_id);
    let content = record.to_content();
    let write_result = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .and_then(|mut file| file.write_all(content.as_bytes()));
    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err);
    }
    let link_result = std::fs::hard_link(&temp_path, lock_path);
    let _ = std::fs::remove_file(&temp_path);
    link_result
}

fn remove_stale_lock<F>(lock_path: &Path, after_rename: F) -> std::result::Result<(), String>
where
    F: FnOnce(),
{
    let cleanup_path = lock_path.with_extension(unique_lock_extension("malformed"));
    match std::fs::rename(lock_path, &cleanup_path) {
        Ok(()) => {
            after_rename();
            let _ = std::fs::remove_file(&cleanup_path);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

fn write_lock_record(lock_path: &Path, record: &LockRecord) -> std::io::Result<()> {
    let temp_path = lock_path.with_extension(unique_lock_extension("tmp"));
    std::fs::write(&temp_path, record.to_content())?;
    std::fs::rename(temp_path, lock_path)
}

fn read_lock_record(lock_path: &Path) -> Option<LockRecord> {
    LockRecord::parse(&std::fs::read_to_string(lock_path).ok()?)
}

fn sweep_orphan_lock_files(wt_path: &Path) {
    let Ok(entries) = std::fs::read_dir(wt_path) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with(".aid-lock.tmp.")
            || name_str.starts_with(".aid-lock.malformed.") {
                let _ = std::fs::remove_file(entry.path());
            }
    }
}

fn task_status_keeps_lock(store: Option<&Store>, task_id: &str) -> bool {
    let Some(store) = store else { return false };
    match store.get_task(task_id) {
        Ok(Some(task)) => !task.status.is_terminal(),
        Ok(None) => false,
        Err(_) => true,
    }
}

fn unique_lock_extension(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!(
        "{prefix}.{}.{}.{}",
        std::process::id(),
        nanos,
        LOCK_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

impl LockRecord {
    fn new(task_id: &str) -> Self {
        Self {
            task_id: task_id.to_string(),
            owner_pid: Some(std::process::id()),
            worker_pid: None,
            generation: 1,
        }
    }

    fn parse(content: &str) -> Option<Self> {
        let mut task_id = None;
        let mut owner_pid = None;
        let mut worker_pid = None;
        let mut generation = None;
        for line in content.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key {
                "task_id" if !value.is_empty() => task_id = Some(value.to_string()),
                "owner_pid" => owner_pid = value.parse::<u32>().ok(),
                "worker_pid" => worker_pid = value.parse::<u32>().ok(),
                "generation" => generation = value.parse::<u64>().ok(),
                _ => {}
            }
        }
        Some(Self { task_id: task_id?, owner_pid, worker_pid, generation: generation.unwrap_or(1) })
    }

    fn to_content(&self) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let worker_pid = self.worker_pid.map_or(String::new(), |pid| pid.to_string());
        format!(
            "version=1\ntask_id={}\nowner_pid={}\nworker_pid={}\ngeneration={}\nupdated_at={}\n",
            self.task_id,
            self.owner_pid.map_or(String::new(), |pid| pid.to_string()),
            worker_pid,
            self.generation,
            now
        )
    }
}

#[cfg(test)]
pub(crate) fn simulate_stale_recovery_race(
    wt_path: &Path,
    task_id: &str,
    after_stale_rename: impl FnOnce(),
) -> std::result::Result<(), String> {
    try_acquire_worktree_lock_with_hook(wt_path, task_id, None, after_stale_rename)
}
