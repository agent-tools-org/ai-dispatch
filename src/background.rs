// Detached background worker support for aid tasks.
// Persists run specs under ~/.aid/jobs and re-execs the binary to finish work.

use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use crate::agent::{self, RunOpts};
use crate::config;
use crate::notify;
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, TaskEvent, TaskFilter, TaskId, TaskStatus};

const ZOMBIE_FAILURE_DETAIL: &str = "Background worker died unexpectedly";

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
    pub max_duration_mins: Option<i64>,
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
    pub parent_task_id: Option<String>,
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

pub fn spawn_worker(task_id: &str) -> Result<Child> {
    let exe = std::env::current_exe().context("Failed to resolve current aid binary")?;
    Command::new(exe)
        .args(["__run-task", task_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to spawn detached background worker")
}

pub async fn run_task(store: Arc<Store>, task_id: &str) -> Result<()> {
    let spec = load_spec(task_id)?;
    let result = run_task_inner(&store, &spec).await;
    let _ = remove_spec(task_id);
    let _ = crate::input_signal::clear_response(task_id);

    if let Err(err) = result {
        record_worker_failure(&store, task_id, &err)?;
        crate::webhook::fire_task_webhooks(&store, task_id).await;
        if let Some(ref cmd) = spec.on_done {
            let _ = std::process::Command::new("sh")
                .args(["-c", cmd])
                .env("AID_TASK_ID", task_id)
                .env("AID_TASK_STATUS", "failed")
                .spawn();
        }
        return Err(err);
    }

    crate::webhook::fire_task_webhooks(&store, task_id).await;

    if let Some(ref cmd) = spec.on_done {
        let _ = std::process::Command::new("sh")
            .args(["-c", cmd])
            .env("AID_TASK_ID", task_id)
            .env("AID_TASK_STATUS", "done")
            .spawn();
    }

    Ok(())
}

pub fn check_zombie_tasks(store: &Store) -> Result<Vec<String>> {
    check_zombie_tasks_with(store, is_process_running)
}

pub(crate) fn load_worker_pid(task_id: &str) -> Result<Option<u32>> {
    Ok(load_spec_if_exists(task_id)?.and_then(|spec| spec.worker_pid))
}

async fn run_task_inner(store: &Arc<Store>, spec: &BackgroundRunSpec) -> Result<()> {
    let agent_kind = AgentKind::parse_str(&spec.agent_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown agent '{}'", spec.agent_name))?;
    let agent = agent::get_agent(agent_kind);
    let opts = RunOpts {
        dir: spec.dir.clone(),
        output: spec.output.clone(),
        model: spec.model.clone(),
        budget: false,
        read_only: false,
    };
    let mut std_cmd = agent
        .build_command(&spec.prompt, &opts)
        .context("Failed to build agent command")?;
    if agent::is_rust_project(spec.dir.as_deref())
        && let Some(target_dir) = agent::shared_target_dir()
    {
        std_cmd.env("CARGO_TARGET_DIR", &target_dir);
    }
    if spec.interactive {
        crate::pty_runner::run_agent_process(
            &*agent,
            &std_cmd,
            &TaskId(spec.task_id.clone()),
            store,
            &paths::log_path(&spec.task_id),
            spec.output.as_deref(),
            spec.model.as_deref(),
            agent.streaming(),
        )?;
    } else {
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
    }
    crate::cmd::run::maybe_verify(
        store,
        &TaskId(spec.task_id.clone()),
        spec.verify.as_deref(),
        spec.dir.as_deref(),
    );
    if let Some(task) = store.get_task(&spec.task_id)? {crate::cmd::run::maybe_cleanup_fast_fail(store, &TaskId(spec.task_id.clone()), &task);
    }
    notify_task_completion(store, &spec.task_id)?;
    if let Some(worktree_dir) = spec.dir.as_deref() {
        if crate::commit::has_uncommitted_changes(worktree_dir).unwrap_or(false) {
            if let Err(e) = crate::commit::auto_commit(worktree_dir, &spec.task_id, &spec.prompt) {
                eprintln!("[aid] auto-commit failed: {e}");
            }
        }
    }
    if let Some(mut retry_args) = crate::cmd::retry_logic::prepare_retry(
        store.clone(),
        &TaskId(spec.task_id.clone()),
        &crate::cmd::run::RunArgs {
            agent_name: spec.agent_name.clone(),
            prompt: spec.prompt.clone(),
            repo: None,
            dir: spec.dir.clone(),
            output: spec.output.clone(),
            model: spec.model.clone(),
            worktree: None,
            base_branch: None,
            group: spec.group.clone(),
            verify: spec.verify.clone(),
            max_duration_mins: spec.max_duration_mins,
            retry: spec.retry,
            context: vec![],
            skills: spec.skills.clone(),
            template: spec.template.clone(),
            background: false,
            announce: false,
            parent_task_id: spec.parent_task_id.clone(),
            on_done: None,
            fallback: None,
            read_only: false,
        },
    )
    .await?
    {
        if let Some(task) = store.get_task(&spec.task_id)? {
            crate::cmd::run::inherit_retry_base_branch(spec.dir.as_deref(), &task, &mut retry_args);
        }
        Box::pin(crate::cmd::run::run(store.clone(), retry_args)).await?;
    }

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

pub(crate) fn clear_spec(task_id: &str) -> Result<()> {
    remove_spec(task_id)
}

pub(crate) fn update_worker_pid(task_id: &str, worker_pid: u32) -> Result<()> {
    let mut spec = load_spec(task_id)?;
    spec.worker_pid = Some(worker_pid);
    save_spec(&spec)
}

fn record_worker_failure(store: &Store, task_id: &str, err: &anyhow::Error) -> Result<()> {
    record_failure(
        store,
        task_id,
        &format!("{err:#}"),
        &format!("Background worker failed: {err}"),
    )
}

fn check_zombie_tasks_with<F>(store: &Store, is_worker_alive: F) -> Result<Vec<String>>
where
    F: Fn(u32) -> bool,
{
    let config = config::load_config()?;
    let mut cleaned = Vec::new();
    for task in store.list_tasks(TaskFilter::Running)? {
        let task_id = task.id.as_str();
        let Some(spec) = load_spec_if_exists(task_id)? else {
            continue;
        };
        let Some(worker_pid) = spec.worker_pid else {
            if let Some(task) = store.get_task(task_id)? {
                if let Some(ref path) = task.worktree_path {
                    if std::path::Path::new(path).exists() && crate::commit::has_uncommitted_changes(path).unwrap_or(false) {
                        let _ = crate::commit::auto_commit(path, task_id, &task.prompt);
                        eprintln!("[aid] Preserved uncommitted changes for zombie task {task_id}");
                    }
                }
            }
            record_failure(store, task_id, ZOMBIE_FAILURE_DETAIL, ZOMBIE_FAILURE_DETAIL)?;
            cleaned.push(task_id.to_string());
            continue;
        };
        if is_worker_alive(worker_pid) {
            let elapsed_mins = (Local::now() - task.created_at).num_minutes();
            let max_duration_mins = spec
                .max_duration_mins
                .unwrap_or(config.background.max_task_duration_mins);
            if elapsed_mins > max_duration_mins {
                kill_process(worker_pid);
                let detail = format!(
                    "Task exceeded max duration ({}m > {}m)",
                    elapsed_mins, max_duration_mins
                );
                record_failure(store, task_id, &detail, &detail)?;
                cleaned.push(task_id.to_string());
            }
            continue;
        }

        if let Some(task) = store.get_task(task_id)? {
            if let Some(ref path) = task.worktree_path {
                if std::path::Path::new(path).exists() && crate::commit::has_uncommitted_changes(path).unwrap_or(false) {
                    let _ = crate::commit::auto_commit(path, task_id, &task.prompt);
                    eprintln!("[aid] Preserved uncommitted changes for zombie task {task_id}");
                }
            }
        }
        record_failure(store, task_id, ZOMBIE_FAILURE_DETAIL, ZOMBIE_FAILURE_DETAIL)?;
        cleaned.push(task_id.to_string());
    }
    Ok(cleaned)
}

fn load_spec_if_exists(task_id: &str) -> Result<Option<BackgroundRunSpec>> {
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

fn record_failure(
    store: &Store,
    task_id: &str,
    stderr_detail: &str,
    event_detail: &str,
) -> Result<()> {
    let stderr_path = paths::stderr_path(task_id);
    std::fs::write(&stderr_path, format!("{stderr_detail}\n"))?;
    store.update_task_status(task_id, TaskStatus::Failed)?;
    store.insert_event(&TaskEvent {
        task_id: TaskId(task_id.to_string()),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail: event_detail.to_string(),
        metadata: None,
    })?;
    notify_task_completion(store, task_id)?;
    Ok(())
}

fn notify_task_completion(store: &Store, task_id: &str) -> Result<()> {
    if let Some(task) = store.get_task(task_id)? {
        notify::notify_completion(&task);
    }
    Ok(())
}

#[cfg(unix)]
fn kill_process(pid: u32) {
    if pid > i32::MAX as u32 {
        return;
    }
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid as i32, 15) };
}

#[cfg(not(unix))]
fn kill_process(_pid: u32) {}

#[cfg(unix)]
fn is_process_running(pid: u32) -> bool {
    if pid > i32::MAX as u32 {
        return false;
    }

    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }

    let result = unsafe { kill(pid as i32, 0) };
    if result != 0 && std::io::Error::last_os_error().raw_os_error() != Some(1) {
        return false;
    }

    if !is_process_not_zombie(pid) {
        return false;
    }

    true
}

#[cfg(unix)]
fn is_process_not_zombie(pid: u32) -> bool {
    use std::process::Command;
    const WNOHANG: i32 = 1;

    unsafe extern "C" {
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }

    if let Ok(output) = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
    {
        let stat = String::from_utf8_lossy(&output.stdout);
        if !stat.trim().is_empty() {
            return !stat.trim().starts_with('Z');
        }
    }
    let mut status = 0;
    unsafe { waitpid(pid as i32, &mut status, WNOHANG) <= 0 }
}

#[cfg(not(unix))]
fn is_process_running(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests;
