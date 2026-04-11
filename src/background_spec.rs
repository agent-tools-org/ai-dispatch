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
    pub model: Option<String>,
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
    pub max_duration_mins: Option<i64>,
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    pub retry: u32,
    pub group: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub checklist: Vec<String>,
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
    pub read_only: bool,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default = "default_link_deps")]
    pub link_deps: bool,
}

fn default_link_deps() -> bool { true }

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

#[cfg(test)]
mod tests {
    use super::BackgroundRunSpec;
    use std::collections::HashMap;

    fn sample_spec(read_only: bool) -> BackgroundRunSpec {
        BackgroundRunSpec {
            task_id: "t-1234".to_string(),
            worker_pid: Some(11),
            agent_name: "codex".to_string(),
            prompt: "fix the bug".to_string(),
            dir: Some("/tmp/project".to_string()),
            output: Some("json".to_string()),
            result_file: Some("result.md".to_string()),
            model: Some("gpt-5.4".to_string()),
            verify: Some("cargo check".to_string()),
            setup: Some("cargo fetch".to_string()),
            iterate: Some(3),
            eval: Some("cargo test".to_string()),
            eval_feedback_template: Some("Iteration {iteration}/{max_iterations}".to_string()),
            judge: Some("cursor".to_string()),
            max_duration_mins: Some(15),
            idle_timeout_secs: Some(60),
            retry: 2,
            group: Some("core".to_string()),
            skills: vec!["ai-coding".to_string()],
            checklist: vec!["confirm retry path".to_string()],
            template: Some("default".to_string()),
            interactive: true,
            on_done: Some("echo done".to_string()),
            cascade: vec!["notify".to_string()],
            parent_task_id: Some("t-parent".to_string()),
            env: Some(HashMap::from([("KEY".to_string(), "VALUE".to_string())])),
            env_forward: Some(vec!["HOME".to_string()]),
            agent_pid: Some(22),
            sandbox: true,
            read_only,
            container: Some("aid:test".to_string()),
            link_deps: true,
        }
    }

    #[test]
    fn background_run_spec_round_trips_read_only() {
        let value = serde_json::to_value(sample_spec(true)).unwrap();
        assert_eq!(value.get("read_only").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(value.get("result_file").and_then(|v| v.as_str()), Some("result.md"));
        assert_eq!(value.get("iterate").and_then(|v| v.as_u64()), Some(3));

        let decoded: BackgroundRunSpec = serde_json::from_value(value).unwrap();
        assert!(decoded.read_only);
        assert_eq!(decoded.result_file.as_deref(), Some("result.md"));
        assert_eq!(decoded.eval.as_deref(), Some("cargo test"));
    }

    #[test]
    fn background_run_spec_defaults_read_only_to_false_when_missing() {
        let mut value = serde_json::to_value(sample_spec(false)).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("read_only");

        let decoded: BackgroundRunSpec = serde_json::from_value(value).unwrap();
        assert!(!decoded.read_only);
    }
}
