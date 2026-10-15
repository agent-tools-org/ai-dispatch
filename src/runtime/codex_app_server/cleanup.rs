// Recover task-scoped Codex state left by killed workers on the next launch.
// PID start identities protect live workers and unrelated processes from cleanup.

use crate::runtime::process::monitor::activity;
use crate::support::paths;
use anyhow::{Context, Result};
use serde_json::{Value, json};

pub(super) struct Owner(std::path::PathBuf);

impl Owner {
    pub fn create(task: &str) -> Result<Self> {
        let path = paths::task_dir(task).join("codex-owner.json");
        let pid = std::process::id();
        let started =
            activity::root_started(pid).context("Cannot identify Codex worker process")?;
        use std::io::Write;
        let temporary = path.with_extension(format!(
            "{pid}-{}.tmp",
            chrono::Local::now()
                .timestamp_nanos_opt()
                .context("Cannot timestamp Codex worker lease")?
        ));
        let result = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(json!({"pid":pid,"started":started}).to_string().as_bytes())?;
            std::fs::hard_link(&temporary, &path)?;
            Ok(())
        })();
        let _ = std::fs::remove_file(temporary);
        result?;
        Ok(Self(path))
    }

    pub fn server_marker(&self, task: &str, pid: u32) -> Result<Option<super::control::Active>> {
        let dir = self.0.parent().context("Missing Codex owner directory")?;
        if dir == paths::task_dir(task) {
            return Ok(None);
        }
        let origin = dir
            .file_name()
            .and_then(|name| name.to_str())
            .context("Invalid Codex origin task")?;
        super::control::Active::starting(origin, pid).map(Some)
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn identity(path: &std::path::Path) -> Option<(u32, String)> {
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some((
        u32::try_from(value["pid"].as_u64()?).ok()?,
        value["started"].as_str()?.to_string(),
    ))
}

pub(super) fn sweep() -> Result<()> {
    let tasks = paths::aid_dir().join("tasks");
    for entry in std::fs::read_dir(tasks)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        sweep_task(&entry.path())?;
    }
    Ok(())
}

fn sweep_task(task: &std::path::Path) -> Result<()> {
    let owner = task.join("codex-owner.json");
    let marker = task.join("codex-app-server.json");
    if identity(&owner).is_some_and(|(pid, start)| {
        activity::root_matches(pid, Some(&start))
            && crate::runtime::background::is_process_running(pid)
    }) {
        return Ok(());
    }
    if let Some((pid, start)) = identity(&marker) {
        if activity::root_matches(pid, Some(&start)) {
            // A marker from the earlier transport has no worker identity: preserve it.
            if !owner.exists() {
                return Ok(());
            }
            #[cfg(unix)]
            crate::runtime::process::group::kill_with_grace(
                pid as i32,
                std::time::Duration::from_millis(100),
            );
        }
    }
    let home = task.join("codex-home");
    if home.symlink_metadata().is_ok() {
        std::fs::remove_dir_all(home)?;
    }
    for path in [marker, owner] {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
