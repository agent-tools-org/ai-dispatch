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

/// Returns /tmp/aid-wg-{id}/ as the workspace directory for a workgroup.
pub fn workspace_dir(workgroup_id: &str) -> Result<PathBuf> {
    sanitize::validate_workgroup_id(workgroup_id)?;
    Ok(std::path::PathBuf::from(format!("/tmp/aid-wg-{workgroup_id}")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_uses_tmp() {
        let path = workspace_dir("wg-abcd").unwrap();
        assert_eq!(path.to_str().unwrap(), "/tmp/aid-wg-wg-abcd");
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
}
