// Filesystem paths for aid: ~/.aid/ directory, logs, database.
// Centralizes all path logic so nothing hardcodes paths.

use anyhow::Result;
use std::path::PathBuf;

pub fn aid_dir() -> PathBuf {
    dirs_home().join(".aid")
}

pub fn logs_dir() -> PathBuf {
    aid_dir().join("logs")
}

pub fn db_path() -> PathBuf {
    aid_dir().join("aid.db")
}

pub fn log_path(task_id: &str) -> PathBuf {
    logs_dir().join(format!("{task_id}.jsonl"))
}

pub fn ensure_dirs() -> Result<()> {
    std::fs::create_dir_all(logs_dir())?;
    Ok(())
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_under_aid_dir() {
        let base = aid_dir();
        assert!(db_path().starts_with(&base));
        assert!(logs_dir().starts_with(&base));
        assert!(log_path("t-1234").starts_with(&base));
    }
}
