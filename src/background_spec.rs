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
    pub model: Option<String>,
    pub verify: Option<String>,
    #[serde(default)]
    pub judge: Option<String>,
    #[serde(default)]
    pub max_duration_mins: Option<i64>,
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    pub retry: u32,
    pub group: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub template: Option<String>,
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
    pub container: Option<String>,
}

pub fn save_spec(spec: &BackgroundRunSpec) -> Result<()> {
    sanitize::validate_task_id(&spec.task_id)?;
    let path = paths::job_path(&spec.task_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(spec)?;
    std::fs::write(path, content)?;
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

pub(crate) fn load_spec_if_exists(task_id: &str) -> Result<Option<BackgroundRunSpec>> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::job_path(task_id);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read background spec {}", path.display()))?;
    let spec = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse background spec {}", path.display()))?;
    Ok(Some(spec))
}
