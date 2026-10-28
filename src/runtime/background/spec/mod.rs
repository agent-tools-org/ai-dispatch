// Background spec persistence for aid task workers.
// Exports the serialized run spec plus read/write helpers for ~/.aid/jobs state.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::support::paths;
use crate::support::sanitize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundRunSpec {
    pub task_id: String,
    pub worker_pid: Option<u32>,
    pub dir: Option<String>,
    #[serde(default)]
    pub budget: bool,
    pub max_duration_mins: Option<i64>,
    pub max_duration_secs: Option<u64>,
    pub idle_timeout_secs: Option<u64>,
    #[serde(default)]
    pub interactive: bool,
    pub on_done: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub agent_pid: Option<u32>,
    pub agent_started: Option<String>,
    #[serde(default)]
    pub audit_report_mode: bool,
    pub pre_task_dirty_paths: Option<Vec<String>>,
    #[serde(default)]
    pub foreground: bool,
}

impl BackgroundRunSpec {
    pub(crate) fn prepare_agent(task_id: &str) -> Result<()> {
        if load_spec_if_exists(task_id)?.is_some() {
            return Ok(());
        }
        save_spec(&Self {
            task_id: task_id.to_string(),
            worker_pid: None,
            dir: None,
            budget: false,
            max_duration_mins: None,
            max_duration_secs: None,
            idle_timeout_secs: None,
            interactive: false,
            on_done: None,
            env: None,
            agent_pid: None,
            agent_started: None,
            audit_report_mode: false,
            pre_task_dirty_paths: None,
            foreground: false,
        })
    }

    pub(crate) fn agent_pid_for_signal(&self) -> Option<u32> {
        let pid = self.agent_pid?;
        if crate::runtime::process::monitor::activity::root_matches(pid, self.agent_started.as_deref()) {
            return Some(pid);
        }
        aid_warn!(
            "[aid] Skip agent signal for {} (pid {pid}): launch identity missing, unreadable or mismatched",
            self.task_id,
        );
        None
    }
}

pub fn save_spec(spec: &BackgroundRunSpec) -> Result<()> {
    #[cfg(test)]
    SAVE_HOOK.with(|hook| {
        if let Some(prepare) = hook.borrow_mut().take() {
            prepare();
        }
    });
    sanitize::validate_task_id(&spec.task_id)?;
    let path = paths::job_path(&spec.task_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(spec)?;
    // Pollers must see a complete old or new spec, never a truncated PID update.
    let temp = path.with_extension(format!("{:032x}.tmp", rand::random::<u128>()));
    let result = write_private(&temp, content.as_bytes()).and_then(|()| std::fs::rename(&temp, &path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result?;
    Ok(())
}

/// A spec carries the task's inline env values, so only the owner may read it.
fn write_private(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(content)
}

pub(crate) fn load_spec(task_id: &str) -> Result<BackgroundRunSpec> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::job_path(task_id);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read background spec {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse background spec {}", path.display()))
}

pub(crate) fn remove_spec(task_id: &str) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::job_path(task_id);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub(crate) fn clear_spec(task_id: &str) -> Result<()> {
    remove_spec(task_id)
}

pub fn load_worker_pid(task_id: &str) -> Result<Option<u32>> {
    sanitize::validate_task_id(task_id)?;
    Ok(load_spec_if_exists(task_id)?.and_then(|spec| spec.worker_pid))
}

pub fn load_spec_if_exists(task_id: &str) -> Result<Option<BackgroundRunSpec>> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::job_path(task_id);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("Failed to read background spec {}", path.display()));
        }
    };
    let spec = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse background spec {}", path.display()))?;
    Ok(Some(spec))
}

pub(crate) fn load_spec_for_reaper(task_id: &str) -> std::result::Result<Option<BackgroundRunSpec>, ()> {
    match load_spec_if_exists(task_id) {
        Ok(spec) => Ok(spec),
        Err(error) => {
            aid_warn!("[aid] Skipping reaper cleanup for {task_id}: {error:#}");
            Err(())
        }
    }
}

#[cfg(test)]
#[path = "spec_roundtrip_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "spec_tests.rs"]
mod legacy_tests;

#[cfg(test)]
thread_local! {
    static SAVE_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
impl BackgroundRunSpec {
    pub(crate) fn on_next_save(hook: impl FnOnce() + 'static) {
        SAVE_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }
}
