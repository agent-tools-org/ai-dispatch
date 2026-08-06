// Worktree lease lock helpers for task dispatch.
// Exports lock acquisition, ownership re-keying, and cleanup checks.
// Deps: Store task status fallback, state process liveness, std fs primitives.

use crate::store::Store;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "lock_reentry.rs"]
mod lock_reentry;

const LOCK_FILENAME: &str = ".aid-lock";
static LOCK_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
struct LockRecord {
    task_id: String,
    owner_pid: Option<u32>,
    worker_pid: Option<u32>,
}

pub fn check_worktree_lock(wt_path: &Path) -> Option<String> {
    check_worktree_lock_with_store(wt_path, None)
}

// Read-only check; stale removal is acquisition-only to avoid racing live leases.
pub fn check_worktree_lock_with_store(wt_path: &Path, store: Option<&Store>) -> Option<String> {
    let record = read_lock_record(&wt_path.join(LOCK_FILENAME))?;
    lock_record_is_held(&record, store).then_some(record.task_id)
}

// Store-aware liveness: pid-based when a pid is recorded, otherwise falls back
// to whether the holder task is still non-terminal. Matches the acquisition
// gate in lock_record_is_held, so a retry that proceeds because the holder was
// stopped will not be re-refused on the next create_worktree call.
pub(crate) fn live_lock_holder_with_store(wt_path: &Path, store: &Store) -> Option<String> {
    let record = read_lock_record(&wt_path.join(LOCK_FILENAME))?;
    lock_record_is_held(&record, Some(store)).then_some(record.task_id)
}

fn lock_record_is_held(record: &LockRecord, store: Option<&Store>) -> bool {
    if let Some(worker_pid) = record.worker_pid {
        return super::state::process_alive(worker_pid);
    }
    if let Some(owner_pid) = record.owner_pid
        && super::state::process_alive(owner_pid)
    {
        return true;
    }
    // Dead/missing owner without worker_pid: ambiguous re-key window. Store
    // decides; without one, treat as held.
    match store {
        Some(store) => task_status_keeps_lock(store, &record.task_id),
        None => true,
    }
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
    try_acquire_worktree_lock_with_hooks(wt_path, task_id, store, || {}, || {})
}

fn try_acquire_worktree_lock_with_hooks<B, A>(
    wt_path: &Path,
    task_id: &str,
    store: Option<&Store>,
    before_stale_rename: B,
    after_stale_rename: A,
) -> std::result::Result<(), String>
where
    B: FnOnce(),
    A: FnOnce(),
{
    let lock_path = wt_path.join(LOCK_FILENAME);
    let mut hooks = Some((before_stale_rename, after_stale_rename));
    for attempt in 0..2 {
        match create_lock_file(&lock_path, task_id) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let Ok(content) = std::fs::read_to_string(&lock_path) else {
                    if attempt == 0 {
                        continue; // holder vanished between create and read; retry
                    }
                    return Err("unknown".to_string());
                };
                let holder = LockRecord::parse(&content);
                if let Some(record) = &holder
                    && lock_record_is_held(record, store)
                {
                    if lock_reentry::holder_allows_reentry(store, task_id, &record.task_id) {
                        return Ok(());
                    }
                    return Err(record.task_id.clone());
                }
                if attempt == 0 {
                    if let Some((before, after)) = hooks.take() {
                        remove_stale_lock(&lock_path, &content, before, after)?;
                    }
                    continue;
                }
                return Err(holder
                    .map(|record| record.task_id)
                    .unwrap_or_else(|| "unknown".to_string()));
            }
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("unknown".to_string())
}

pub fn clear_worktree_lock(
    wt_path: &Path,
    expected_task_id: &str,
) -> std::result::Result<(), String> {
    let lock_path = wt_path.join(LOCK_FILENAME);
    match read_lock_record(&lock_path) {
        Some(record) if record.task_id == expected_task_id => {
            let _ = std::fs::remove_file(&lock_path);
        }
        Some(record) => return Err(record.task_id),
        // Missing: ok. Malformed: leave for acquisition recovery.
        None => {}
    }
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

// Drop a stale lock; restore if a competitor wrote a fresh lease mid-rename.
fn remove_stale_lock<B, A>(
    lock_path: &Path,
    stale_content: &str,
    before_rename: B,
    after_rename: A,
) -> std::result::Result<(), String>
where
    B: FnOnce(),
    A: FnOnce(),
{
    before_rename();
    let cleanup_path = lock_path.with_extension(unique_lock_extension("malformed"));
    match std::fs::rename(lock_path, &cleanup_path) {
        Ok(()) => {
            after_rename();
            let captured = std::fs::read_to_string(&cleanup_path).unwrap_or_default();
            if captured == stale_content {
                let _ = std::fs::remove_file(&cleanup_path);
                return Ok(());
            }
            // hard_link never clobbers; a post-rename winner keeps its lease.
            if std::fs::hard_link(&cleanup_path, lock_path).is_ok() {
                let _ = std::fs::remove_file(&cleanup_path);
            }
            Err(LockRecord::parse(&captured)
                .map(|record| record.task_id)
                .unwrap_or_else(|| "unknown".to_string()))
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

pub(super) fn live_lock_holder(wt_path: &Path) -> Option<String> {
    let record = read_lock_record(&wt_path.join(LOCK_FILENAME))?;
    let pid = record.worker_pid.or(record.owner_pid)?;
    super::state::process_alive(pid).then_some(record.task_id)
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

fn task_status_keeps_lock(store: &Store, task_id: &str) -> bool {
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
        }
    }

    fn parse(content: &str) -> Option<Self> {
        let mut task_id = None;
        let mut owner_pid = None;
        let mut worker_pid = None;
        for line in content.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key {
                "task_id" if !value.is_empty() => task_id = Some(value.to_string()),
                "owner_pid" => owner_pid = value.parse::<u32>().ok(),
                "worker_pid" => worker_pid = value.parse::<u32>().ok(),
                _ => {}
            }
        }
        Some(Self { task_id: task_id?, owner_pid, worker_pid })
    }

    fn to_content(&self) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        format!(
            "version=1\ntask_id={}\nowner_pid={}\nworker_pid={}\nupdated_at={}\n",
            self.task_id,
            self.owner_pid.map_or(String::new(), |pid| pid.to_string()),
            self.worker_pid.map_or(String::new(), |pid| pid.to_string()),
            now
        )
    }
}

#[cfg(test)]
pub(crate) fn simulate_stale_recovery_race(
    wt_path: &Path,
    task_id: &str,
    before_stale_rename: impl FnOnce(),
    after_stale_rename: impl FnOnce(),
) -> std::result::Result<(), String> {
    try_acquire_worktree_lock_with_hooks(wt_path, task_id, None, before_stale_rename, after_stale_rename)
}
