// Filesystem paths for aid: ~/.aid/ directory, logs, database.
// Centralizes all path logic so nothing hardcodes paths.

use anyhow::Result;
#[cfg(test)]
use std::ffi::OsString;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{LazyLock, Mutex};

pub fn aid_dir() -> PathBuf {
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

pub fn log_path(task_id: &str) -> PathBuf {
    logs_dir().join(format!("{task_id}.jsonl"))
}

pub fn stderr_path(task_id: &str) -> PathBuf {
    logs_dir().join(format!("{task_id}.stderr"))
}

pub fn job_path(task_id: &str) -> PathBuf {
    jobs_dir().join(format!("{task_id}.json"))
}

pub fn job_input_path(task_id: &str) -> PathBuf {
    jobs_dir().join(format!("{task_id}.input"))
}

/// Returns /tmp/aid-wg-{id}/ as the workspace directory for a workgroup.
pub fn workspace_dir(workgroup_id: &str) -> PathBuf {
    std::path::PathBuf::from(format!("/tmp/aid-wg-{workgroup_id}"))
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
pub static AID_HOME_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[cfg(test)]
pub struct AidHomeGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<OsString>,
}

#[cfg(test)]
impl AidHomeGuard {
    pub fn set(path: &std::path::Path) -> Self {
        let lock = AID_HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("AID_HOME");
        unsafe { std::env::set_var("AID_HOME", path) };
        Self {
            _lock: lock,
            previous,
        }
    }
}

#[cfg(test)]
impl Drop for AidHomeGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(path) => unsafe { std::env::set_var("AID_HOME", path) },
            None => unsafe { std::env::remove_var("AID_HOME") },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_uses_tmp() {
        let path = workspace_dir("wg-test");
        assert_eq!(path.to_str().unwrap(), "/tmp/aid-wg-wg-test");
    }

    #[test]
    fn paths_are_under_aid_dir() {
        let _lock = AID_HOME_LOCK.lock().unwrap();
        let base = aid_dir();
        assert!(db_path().starts_with(&base));
        assert!(config_path().starts_with(&base));
        assert!(jobs_dir().starts_with(&base));
        assert!(logs_dir().starts_with(&base));
        assert!(job_path("t-1234").starts_with(&base));
        assert!(job_input_path("t-1234").starts_with(&base));
        assert!(log_path("t-1234").starts_with(&base));
    }
}
