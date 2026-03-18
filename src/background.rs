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
use crate::sanitize;
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
    pub judge: Option<String>,
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
    sanitize::validate_task_id(&spec.task_id)?;
    let path = paths::job_path(&spec.task_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(spec)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn spawn_worker(task_id: &str) -> Result<Child> {
    sanitize::validate_task_id(task_id)?;
    let exe = std::env::current_exe().context("Failed to resolve current aid binary")?;
    let mut cmd = Command::new(exe);
    cmd.args(["__run-task", task_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Create a new process group so we can kill the worker and all its children.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
        .context("Failed to spawn detached background worker")
}

pub async fn run_task(store: Arc<Store>, task_id: &str) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
    let spec = load_spec(task_id)?;
    let result = run_task_inner(&store, &spec).await;
    let _ = remove_spec(task_id);
    let _ = crate::input_signal::clear_response(task_id);
    let _ = crate::input_signal::clear_steer(task_id);

    if let Err(err) = result {
        record_worker_failure(&store, task_id, &err)?;
        crate::webhook::fire_task_webhooks(&store, task_id).await;
        if let Some(ref cmd) = spec.on_done {
            let _ = spawn_on_done_command(cmd, task_id, "failed");
        }
        return Err(err);
    }

    crate::webhook::fire_task_webhooks(&store, task_id).await;

    if let Some(ref cmd) = spec.on_done {
        let _ = spawn_on_done_command(cmd, task_id, "done");
    }

    Ok(())
}

pub fn check_zombie_tasks(store: &Store) -> Result<Vec<String>> {
    check_zombie_tasks_with(store, is_process_running)
}

pub fn load_worker_pid(task_id: &str) -> Result<Option<u32>> {
    sanitize::validate_task_id(task_id)?;
    Ok(load_spec_if_exists(task_id)?.and_then(|spec| spec.worker_pid))
}

async fn run_task_inner(store: &Arc<Store>, spec: &BackgroundRunSpec) -> Result<()> {
    let agent: Box<dyn agent::Agent> = if let Some(kind) = AgentKind::parse_str(&spec.agent_name) {
        agent::get_agent(kind)
    } else if let Some(custom) = agent::registry::resolve_custom_agent(&spec.agent_name) {
        custom
    } else {
        anyhow::bail!("Unknown agent '{}'", spec.agent_name);
    };
    let opts = RunOpts {
        dir: spec.dir.clone(),
        output: spec.output.clone(),
        model: spec.model.clone(),
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
    };
    let mut std_cmd = agent
        .build_command(&spec.prompt, &opts)
        .context("Failed to build agent command")?;
    let worktree_branch = store
        .get_task(&spec.task_id)?
        .and_then(|task| task.worktree_branch);
    if agent::is_rust_project(spec.dir.as_deref())
        && let Some(target_dir) =
            agent::target_dir_for_worktree(worktree_branch.as_deref())
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
            spec.group.as_deref(),
        )
        .await?;
    }
    let retry_args = crate::cmd::run::RunArgs {
        agent_name: spec.agent_name.clone(),
        prompt: spec.prompt.clone(),
        dir: spec.dir.clone(),
        output: spec.output.clone(),
        model: spec.model.clone(),
        group: spec.group.clone(),
        verify: spec.verify.clone(),
        judge: spec.judge.clone(),
        max_duration_mins: spec.max_duration_mins,
        retry: spec.retry,
        skills: spec.skills.clone(),
        template: spec.template.clone(),
        parent_task_id: spec.parent_task_id.clone(),
        ..Default::default()
    };
    let pre_verify_status = store
        .get_task(&spec.task_id)?
        .map(|task| task.status)
        .unwrap_or(TaskStatus::Done);
    crate::cmd::run::maybe_verify(
        store,
        &TaskId(spec.task_id.clone()),
        spec.verify.as_deref(),
        spec.dir.as_deref(),
    );
    if let Some(task) = store.get_task(&spec.task_id)? {
        crate::cmd::run::maybe_cleanup_fast_fail(store, &TaskId(spec.task_id.clone()), &task);
    }
    if crate::cmd::run::maybe_judge_retry(store, &retry_args, &TaskId(spec.task_id.clone()))
        .await?
        .is_some()
    {
        return Ok(());
    }
    notify_task_completion(store, &spec.task_id)?;
    if let Some(worktree_dir) = spec.dir.as_deref()
        && crate::commit::has_uncommitted_changes(worktree_dir).unwrap_or(false)
        && let Err(e) = crate::commit::auto_commit(worktree_dir, &spec.task_id, &spec.prompt)
    {
        eprintln!("[aid] auto-commit failed: {e}");
    }
    if crate::cmd::run::maybe_auto_retry_after_verify_failure(
        store,
        &TaskId(spec.task_id.clone()),
        &retry_args,
        pre_verify_status,
    )
    .await?
    .is_some()
    {
        return Ok(());
    }
    if let Some(mut retry_args) = crate::cmd::retry_logic::prepare_retry(
        store.clone(),
        &TaskId(spec.task_id.clone()),
        &retry_args,
    )
    .await?
    {
        if let Some(task) = store.get_task(&spec.task_id)? {
            crate::cmd::run::inherit_retry_base_branch(spec.dir.as_deref(), &task, &mut retry_args);
        }
        Box::pin(crate::cmd::run::run(store.clone(), retry_args)).await?;
    } else if let Some(task) = store.get_task(&spec.task_id)?
        && task.status == TaskStatus::Failed
    {
        // Quota cascade: detect quota errors and auto-fallback
        let agent_kind = AgentKind::parse_str(&spec.agent_name);
        if let Some(kind) = agent_kind
            && let Some(message) = crate::cmd::run::read_quota_error_message(&TaskId(spec.task_id.clone()))
        {
            crate::rate_limit::mark_rate_limited(&kind, &message);
            if let Some(fallback) = agent::selection::coding_fallback_for(&kind) {
                eprintln!(
                    "[aid] Quota exhausted for {}, auto-cascading to {}",
                    kind.as_str(),
                    fallback.as_str()
                );
                let mut cascade_args = retry_args;
                cascade_args.agent_name = fallback.as_str().to_string();
                cascade_args.parent_task_id = Some(spec.task_id.clone());
                Box::pin(crate::cmd::run::run(store.clone(), cascade_args)).await?;
            }
        }
    }

    Ok(())
}

fn load_spec(task_id: &str) -> Result<BackgroundRunSpec> {
    sanitize::validate_task_id(task_id)?;
    let path = paths::job_path(task_id);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read background spec {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse background spec {}", path.display()))
}

fn remove_spec(task_id: &str) -> Result<()> {
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
            if let Some(task) = store.get_task(task_id)?
                && let Some(ref path) = task.worktree_path
                && std::path::Path::new(path).exists()
                && crate::commit::has_uncommitted_changes(path).unwrap_or(false)
            {
                let _ = crate::commit::auto_commit(path, task_id, &task.prompt);
                eprintln!("[aid] Preserved uncommitted changes for zombie task {task_id}");
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

        if let Some(task) = store.get_task(task_id)?
            && let Some(ref path) = task.worktree_path
            && std::path::Path::new(path).exists()
            && crate::commit::has_uncommitted_changes(path).unwrap_or(false)
        {
            let _ = crate::commit::auto_commit(path, task_id, &task.prompt);
            eprintln!("[aid] Preserved uncommitted changes for zombie task {task_id}");
        }
        record_failure(store, task_id, ZOMBIE_FAILURE_DETAIL, ZOMBIE_FAILURE_DETAIL)?;
        cleaned.push(task_id.to_string());
    }
    Ok(cleaned)
}

fn load_spec_if_exists(task_id: &str) -> Result<Option<BackgroundRunSpec>> {
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

fn record_failure(
    store: &Store,
    task_id: &str,
    stderr_detail: &str,
    event_detail: &str,
) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
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

fn spawn_on_done_command(command: &str, task_id: &str, status: &str) -> Result<()> {
    let mut cmd = build_on_done_command(command)?;
    cmd.env("AID_TASK_ID", task_id)
        .env("AID_TASK_STATUS", status)
        .spawn()
        .context("failed to spawn on_done callback")?;
    Ok(())
}

fn build_on_done_command(command: &str) -> Result<Command> {
    let mut parts = command.split_whitespace();
    let program = parts.next().context("on_done command is empty")?;
    let args: Vec<&str> = parts.collect();
    let mut cmd = Command::new(program);
    cmd.args(&args);
    Ok(cmd)
}

fn notify_task_completion(store: &Store, task_id: &str) -> Result<()> {
    if let Some(task) = store.get_task(task_id)? {
        notify::notify_completion(&task);
    }
    Ok(())
}

#[cfg(unix)]
pub fn kill_process(pid: u32) {
    if pid > i32::MAX as u32 {
        return;
    }
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // Kill the entire process group (negative PID) first, then the process itself.
    // This ensures agent child processes (git, CLI tools) are also terminated.
    let pid_i32 = pid as i32;
    unsafe {
        kill(-pid_i32, 15); // SIGTERM to process group
        kill(pid_i32, 15); // SIGTERM to process (in case pgid differs)
    };
}

#[cfg(not(unix))]
pub fn kill_process(_pid: u32) {}

#[cfg(unix)]
pub fn sigkill_process(pid: u32) {
    if pid > i32::MAX as u32 {
        return;
    }
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let pid_i32 = pid as i32;
    unsafe {
        kill(-pid_i32, 9); // SIGKILL to process group
        kill(pid_i32, 9); // SIGKILL to process
    };
}

#[cfg(not(unix))]
pub fn sigkill_process(_pid: u32) {}

#[cfg(unix)]
pub fn is_process_running(pid: u32) -> bool {
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
