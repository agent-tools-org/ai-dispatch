// Filesystem paths for aid: ~/.aid/ directory, logs, database.
// Centralizes all path logic so nothing hardcodes paths.

use anyhow::Result;
use std::path::PathBuf;
#[cfg(test)]
use std::cell::RefCell;

use crate::sanitize;

#[cfg(test)]
thread_local! {
    static AID_HOME_OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub fn aid_dir() -> PathBuf {
    #[cfg(test)]
    {
        let maybe = AID_HOME_OVERRIDE.with(|cell| cell.borrow().clone());
        if let Some(p) = maybe {
            return p;
        }
    }
    if let Ok(custom) = std::env::var("AID_HOME") {
        return PathBuf::from(custom);
    }
    dirs_home().join(".aid")
}

pub fn logs_dir() -> PathBuf {
    aid_dir().join("logs")
}

pub fn jobs_dir() -> PathBuf {
    aid_dir().join("jobs")
}

pub fn db_path() -> PathBuf {
    aid_dir().join("aid.db")
}

pub fn config_path() -> PathBuf {
    aid_dir().join("config.toml")
}

pub fn pricing_path() -> PathBuf {
    aid_dir().join("pricing.json")
}

pub fn task_dir(task_id: &str) -> PathBuf {
    aid_dir().join("tasks").join(task_id)
}

pub fn transcript_path(task_id: &str) -> PathBuf {
    task_dir(task_id).join("transcript.md")
}

/// Where an agent that keeps its own log is told to write it, so aid can read that
/// file as proof of life for agents whose stdout stays silent mid-turn.
pub fn agent_log_path(task_id: &str) -> PathBuf {
    task_dir(task_id).join("agent.log")
}

/// The agent-owned output files to watch for proof of life.
/// Used by both the live watcher (pty_watch) and the orphan reaper so they
/// ask the same question about buffered agents.
pub fn agent_byte_paths(task_id: &str) -> [PathBuf; 3] {
    [transcript_path(task_id), log_path(task_id), agent_log_path(task_id)]
}

/// Returns true when any of the agent-owned output files for `task_id` have
/// been written since `started_at`. Used by both the live watcher and the
/// orphan reaper to prove a buffered agent (one that writes nothing to its
/// PTY until it exits) is still alive.
///
/// A file that exists but was last written *before* the task started belongs
/// to an earlier attempt on the same id and is not proof this run did anything.
/// HFS+ truncates mtime to whole seconds; the 2-second grace keeps a file
/// created in the same second as the task from looking stale.
pub fn agent_has_produced_bytes(task_id: &str, started_at: std::time::SystemTime) -> bool {
    let grace = started_at
        .checked_sub(std::time::Duration::from_secs(2))
        .unwrap_or(started_at);
    agent_byte_paths(task_id).into_iter().any(|path| {
        let Ok(meta) = std::fs::metadata(&path) else {
            return false;
        };
        if meta.len() == 0 {
            return false;
        }
        meta.modified().map(|mtime| mtime >= grace).unwrap_or(false)
    })
}

pub fn log_path(task_id: &str) -> PathBuf {
    // Takes a validated task ID from the input boundary.
    logs_dir().join(format!("{task_id}.jsonl"))
}

pub fn stderr_path(task_id: &str) -> PathBuf {
    // Takes a validated task ID from the input boundary.
    logs_dir().join(format!("{task_id}.stderr"))
}

pub fn job_path(task_id: &str) -> PathBuf {
    // Takes a validated task ID from the input boundary.
    jobs_dir().join(format!("{task_id}.json"))
}

pub fn job_input_path(task_id: &str) -> PathBuf {
    // Takes a validated task ID from the input boundary.
    jobs_dir().join(format!("{task_id}.input"))
}

pub fn steer_signal_path(task_id: &str) -> PathBuf {
    // Takes a validated task ID from the input boundary.
    jobs_dir().join(format!("{task_id}.steer"))
}

/// Returns the workspace directory for a workgroup.
///
/// Defaults to `/tmp/aid-wg-{id}/` in production. Under `#[cfg(test)]`, if
/// `AidHomeGuard::set` has activated an override on the current thread, the
/// workspace is rooted under that override instead — so parallel tests never
/// collide on a shared `/tmp/aid-wg-*` path.
pub fn workspace_dir(workgroup_id: &str) -> Result<PathBuf> {
    sanitize::validate_workgroup_id(workgroup_id)?;
    #[cfg(test)]
    {
        let maybe = AID_HOME_OVERRIDE.with(|cell| cell.borrow().clone());
        if let Some(root) = maybe {
            return Ok(root.join("workgroups").join(workgroup_id));
        }
    }
    Ok(PathBuf::from(format!("/tmp/aid-wg-{workgroup_id}")))
}

pub fn ensure_dirs() -> Result<()> {
    std::fs::create_dir_all(logs_dir())?;
    std::fs::create_dir_all(jobs_dir())?;
    Ok(())
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
pub struct AidHomeGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl AidHomeGuard {
    pub fn set(path: &std::path::Path) -> Self {
        let previous = AID_HOME_OVERRIDE.with(|cell| cell.borrow().clone());
        AID_HOME_OVERRIDE.with(|cell| *cell.borrow_mut() = Some(path.to_path_buf()));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for AidHomeGuard {
    fn drop(&mut self) {
        AID_HOME_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous.take());
    }
}

/// Refuse to resolve rate-limit markers (and similar) against the developer's
/// real `~/.aid` when a unit test forgot `AidHomeGuard`.
///
/// # Limitations
///
/// This guard catches same-thread calls through `marker_path`.
/// It does NOT catch:
/// - Anything off-thread (spawned threads or work-stealing async runtime workers
///   do not inherit the `thread_local!` `AidHomeGuard` override, and panics on unjoined
///   threads do not fail tests).
/// - Any deletion or file access that does not go through `marker_path` (or explicit
///   `assert_aid_home_isolated` checks).
///
/// The vanishing-marker bug this was written for is still open: see board item
/// wi-de7e, which records what is established and why bisecting is the wrong next tool.
#[cfg(test)]
pub fn assert_aid_home_isolated(context: &str) {
    let resolved = resolve_path(&aid_dir());
    let real = resolve_path(&dirs_home().join(".aid"));
    if resolved != real {
        return;
    }
    let test_name = std::thread::current()
        .name()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "<unknown test>".to_string());
    panic!(
        "{context} would touch real ~/.aid ({real:?}); \
         set paths::AidHomeGuard in test `{test_name}`"
    );
}

#[cfg(test)]
fn resolve_path(path: &std::path::Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return canon;
    }
    let mut current = path.to_path_buf();
    let mut tail = PathBuf::new();
    while !current.as_os_str().is_empty() {
        if let Ok(canon) = std::fs::canonicalize(&current) {
            return canon.join(tail);
        }
        if let Some(file_name) = current.file_name() {
            tail = PathBuf::from(file_name).join(tail);
            if !current.pop() {
                break;
            }
        } else {
            break;
        }
    }
    let mut normalized = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            c => normalized.push(c.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_uses_tmp_without_override() {
        // No AidHomeGuard active: keep the production /tmp/aid-wg-* path.
        let path = workspace_dir("wg-abcd").unwrap();
        assert_eq!(path.to_str().unwrap(), "/tmp/aid-wg-wg-abcd");
    }

    #[test]
    fn workspace_dir_uses_override_in_tests() {
        let temp = tempfile::TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        let path = workspace_dir("wg-abcd").unwrap();
        assert_eq!(path, temp.path().join("workgroups").join("wg-abcd"));
    }

    #[test]
    fn workspace_dir_rejects_invalid_id() {
        assert!(workspace_dir("wg-../escape").is_err());
        assert!(workspace_dir("not-a-wg").is_err());
    }

    #[test]
    fn paths_are_under_aid_dir() {
        let base = aid_dir();
        assert!(db_path().starts_with(&base));
        assert!(config_path().starts_with(&base));
        assert!(pricing_path().starts_with(&base));
        assert!(jobs_dir().starts_with(&base));
        assert!(logs_dir().starts_with(&base));
        assert!(job_path("t-1234").starts_with(&base));
        assert!(job_input_path("t-1234").starts_with(&base));
        assert!(log_path("t-1234").starts_with(&base));
        assert!(steer_signal_path("t-1234").starts_with(&base));
    }

    #[test]
    fn steer_signal_path_in_jobs() {
        let _guard = AidHomeGuard::set(std::path::Path::new("/tmp/aid-test"));
        let path = steer_signal_path("t-abcd");
        assert!(path.ends_with("jobs/t-abcd.steer"));
    }

    #[test]
    fn assert_aid_home_isolated_passes_with_temp_dir() {
        let temp = tempfile::TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(temp.path());
        assert_aid_home_isolated("test_isolated");
    }

    #[test]
    #[should_panic(expected = "test_variant would touch real ~/.aid")]
    fn assert_aid_home_isolated_detects_home_variant() {
        let _guard = AidHomeGuard::set(&dirs_home().join(".aid").join("."));
        assert_aid_home_isolated("test_variant");
    }
}
