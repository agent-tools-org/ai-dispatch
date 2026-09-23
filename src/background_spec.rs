// Background spec persistence for aid task workers.
// Exports the serialized run spec plus read/write helpers for ~/.aid/jobs state.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::paths;
use crate::sanitize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundRunSpec {
    pub task_id: String,
    pub worker_pid: Option<u32>,
    pub agent_name: String,
    pub prompt: String,
    pub dir: Option<String>,
    pub output: Option<String>,
    #[serde(default)]
    pub result_file: Option<String>,
    #[serde(default)]
    pub result_file_required: Option<bool>,
    pub model: Option<String>,
    #[serde(default)]
    pub budget: bool,
    #[serde(default)]
    pub session_id: Option<String>,
    pub verify: Option<String>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub iterate: Option<u32>,
    #[serde(default)]
    pub eval: Option<String>,
    #[serde(default)]
    pub eval_feedback_template: Option<String>,
    #[serde(default)]
    pub judge: Option<String>,
    #[serde(default)]
    pub judge_retry: bool,
    #[serde(default)]
    pub max_duration_mins: Option<i64>,
    #[serde(default)]
    pub max_duration_secs: Option<u64>,
    #[serde(default)]
    pub max_task_cost: Option<f64>,
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    pub retry: u32,
    pub group: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub checklist: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub peer_review: Option<String>,
    #[serde(default)]
    pub audit: bool,
    #[serde(default)]
    pub audit_explicit: bool,
    #[serde(default)]
    pub no_audit: bool,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub on_done: Option<String>,
    #[serde(default)]
    pub cascade: Vec<String>,
    #[serde(default)]
    pub parent_task_id: Option<String>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub env_forward: Option<Vec<String>>,
    #[serde(default)]
    pub agent_pid: Option<u32>,
    #[serde(default)]
    pub sandbox: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub audit_report_mode: bool,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default = "default_link_deps")]
    pub link_deps: bool,
    #[serde(default)]
    pub pre_task_dirty_paths: Option<Vec<String>>,
    #[serde(default)]
    pub foreground: bool,
}

fn default_link_deps() -> bool { true }

pub fn save_spec(spec: &BackgroundRunSpec) -> Result<()> {
    sanitize::validate_task_id(&spec.task_id)?;
    let path = paths::job_path(&spec.task_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(spec)?;
    // Pollers must see a complete old or new spec, never a truncated PID update.
    let temp = path.with_extension(format!("{:032x}.tmp", rand::random::<u128>()));
    let result = std::fs::write(&temp, content).and_then(|()| std::fs::rename(&temp, &path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result?;
    Ok(())
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
        Err(err) => return Err(err)
            .with_context(|| format!("Failed to read background spec {}", path.display())),
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
#[path = "background_spec_roundtrip_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "background_spec_tests.rs"]
mod legacy_tests;
