// Detached background worker support for aid tasks.
// Persists run specs under ~/.aid/jobs and re-execs the binary to finish work.

use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::Arc;

use crate::agent::{self, RunOpts};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, TaskEvent, TaskId, TaskStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundRunSpec {
    pub task_id: String,
    pub agent_name: String,
    pub prompt: String,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
    pub verify: Option<String>,
    pub retry: u32,
    pub group: Option<String>,
}

pub fn save_spec(spec: &BackgroundRunSpec) -> Result<()> {
    let path = paths::job_path(&spec.task_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(spec)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn spawn_worker(task_id: &str) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to resolve current aid binary")?;
    Command::new(exe)
        .args(["__run-task", task_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn detached background worker")?;
    Ok(())
}

pub async fn run_task(store: Arc<Store>, task_id: &str) -> Result<()> {
    let spec = load_spec(task_id)?;
    let result = run_task_inner(&store, &spec).await;
    let _ = remove_spec(task_id);

    if let Err(err) = result {
        record_worker_failure(&store, task_id, &err)?;
        return Err(err);
    }

    Ok(())
}

async fn run_task_inner(store: &Arc<Store>, spec: &BackgroundRunSpec) -> Result<()> {
    let agent_kind = AgentKind::parse_str(&spec.agent_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown agent '{}'", spec.agent_name))?;
    let agent = agent::get_agent(agent_kind);
    let opts = RunOpts {
        dir: spec.dir.clone(),
        output: spec.output.clone(),
        model: spec.model.clone(),
    };
    let std_cmd = agent.build_command(&spec.prompt, &opts)
        .context("Failed to build agent command")?;
    let mut tokio_cmd = tokio::process::Command::from(std_cmd);
    tokio_cmd.stdout(Stdio::piped());
    tokio_cmd.stderr(Stdio::piped());

    crate::cmd::run::run_agent_process(
        &*agent,
        tokio_cmd,
        &TaskId(spec.task_id.clone()),
        store,
        &paths::log_path(&spec.task_id),
        spec.output.as_deref(),
        spec.model.as_deref(),
        agent.streaming(),
    )
    .await?;
    crate::cmd::run::maybe_verify(
        store,
        &TaskId(spec.task_id.clone()),
        spec.verify.as_deref(),
        spec.dir.as_deref(),
    );
    crate::cmd::run::retry_if_needed(
        store.clone(),
        &TaskId(spec.task_id.clone()),
        &crate::cmd::run::RunArgs {
            agent_name: spec.agent_name.clone(),
            prompt: spec.prompt.clone(),
            dir: spec.dir.clone(),
            output: spec.output.clone(),
            model: spec.model.clone(),
            worktree: None,
            group: spec.group.clone(),
            verify: spec.verify.clone(),
            retry: spec.retry,
            context: vec![],
            background: false,
            parent_task_id: None,
        },
    )
    .await?;

    Ok(())
}

fn load_spec(task_id: &str) -> Result<BackgroundRunSpec> {
    let path = paths::job_path(task_id);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read background spec {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse background spec {}", path.display()))
}

fn remove_spec(task_id: &str) -> Result<()> {
    let path = paths::job_path(task_id);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn record_worker_failure(store: &Store, task_id: &str, err: &anyhow::Error) -> Result<()> {
    let stderr_path = paths::stderr_path(task_id);
    std::fs::write(&stderr_path, format!("{err:#}\n"))?;
    store.update_task_status(task_id, TaskStatus::Failed)?;
    store.insert_event(&TaskEvent {
        task_id: TaskId(task_id.to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: format!("Background worker failed: {err}"),
        metadata: None,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::BackgroundRunSpec;

    #[test]
    fn serializes_spec_to_json() {
        let spec = BackgroundRunSpec {
            task_id: "t-save".to_string(),
            agent_name: "codex".to_string(),
            prompt: "prompt".to_string(),
            dir: Some(".".to_string()),
            output: None,
            model: None,
            verify: Some("auto".to_string()),
            retry: 2,
            group: Some("wg-demo".to_string()),
        };

        let content = serde_json::to_string_pretty(&spec).unwrap();
        assert!(content.contains("\"agent_name\""));
        assert!(content.contains("\"codex\""));
    }
}
