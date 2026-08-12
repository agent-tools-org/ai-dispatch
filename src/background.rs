// Detached background worker support for aid tasks.
// Persists run specs under ~/.aid/jobs and re-execs the binary to finish work.

#[path = "background_process.rs"]
mod background_process;
#[path = "background_lifecycle.rs"]
mod background_lifecycle;
#[path = "background_kill.rs"]
mod background_kill;
#[path = "background_orphan.rs"]
mod background_orphan;
#[path = "background_reaper.rs"]
mod background_reaper;
#[path = "background_spec.rs"]
mod background_spec;
#[path = "background_waiting.rs"]
mod background_waiting;

use anyhow::{Context, Result};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

#[cfg(test)]
use self::background_process::build_on_done_command;
use self::background_process::spawn_on_done_command;
use self::background_reaper::record_worker_failure_skip_notify;
use self::background_spec::{load_spec, remove_spec};
use crate::agent::{self, RunOpts};
use crate::paths;
use crate::sanitize;
use crate::store::Store;
use crate::system_resources;
use crate::types::{AgentKind, TaskFilter, TaskId};

/// Hard limit on concurrent background workers — prevents process exhaustion.
const MAX_WORKERS: usize = 32;

pub use self::background_process::{is_process_running, kill_process, load_agent_pid, sigkill_process, update_agent_pid};
pub use self::background_spec::{load_worker_pid, save_spec, BackgroundRunSpec};
pub(crate) use self::background_process::update_worker_pid;
pub(crate) use self::background_reaper::{check_zombie_tasks_with, record_failure};
#[cfg(test)]
pub(crate) use self::background_reaper::cleanup_stale_pending_tasks;
#[cfg(test)]
pub(crate) use self::background_reaper::{fail_stale_pending_task, ZOMBIE_FAILURE_DETAIL};
pub(crate) use self::background_spec::clear_spec;

pub fn spawn_worker(task_id: &str) -> Result<Child> {
    sanitize::validate_task_id(task_id)?;
    let exe = std::env::current_exe().context("Failed to resolve current aid binary")?;
    let mut cmd = Command::new(exe);
    cmd.args(["__run-task", task_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Inherit AID_HOME so the worker uses the same data directory.
    if let Ok(home) = std::env::var("AID_HOME") {
        cmd.env("AID_HOME", home);
    }
    // Create a new process group so we can kill the worker and all its children.
    // Skip in test context (AID_NO_DETACH=1) so workers die with the test process.
    #[cfg(unix)]
    if std::env::var_os("AID_NO_DETACH").is_none() {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
        .context("Failed to spawn detached background worker")
}

/// Check whether spawning another worker would exceed the process limit.
/// Returns Ok(()) if within limits, Err if at capacity.
pub fn check_worker_capacity(store: &Store) -> Result<()> {
    let running = store.list_tasks(TaskFilter::Running)?.len();
    let soft_limit = system_resources::recommended_max_concurrent();
    if running >= MAX_WORKERS {
        anyhow::bail!("Worker limit reached ({running}/{MAX_WORKERS} active) — wait for tasks to complete");
    }
    if running >= soft_limit {
        aid_warn!("[aid] Warning: {running} active workers (recommended max: {soft_limit})");
    }
    Ok(())
}

pub async fn run_task(store: Arc<Store>, task_id: &str) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
    let spec = load_spec(task_id)?;
    let result = run_task_inner(&store, &spec).await;
    let _ = remove_spec(task_id);
    let _ = crate::input_signal::clear_response(task_id);
    let _ = crate::input_signal::clear_steer(task_id);
    if let Err(err) = result {
        return handle_run_task_inner_error(&store, &spec, err).await;
    }
    if let Some(ref cmd) = spec.on_done {
        let _ = spawn_on_done_command(cmd, task_id, "done");
    }
    Ok(())
}

pub fn check_zombie_tasks(store: &Store) -> Result<Vec<String>> { check_zombie_tasks_with(store, is_process_running) }

async fn handle_run_task_inner_error(
    store: &Arc<Store>,
    spec: &BackgroundRunSpec,
    err: anyhow::Error,
) -> Result<()> {
    let recorded_failure = record_worker_failure_skip_notify(store, &spec.task_id, &err)?;
    if recorded_failure {
        if let Err(lifecycle_err) = run_failed_post_lifecycle(store, spec).await {
            aid_error!("[aid] Background post-run lifecycle failed: {lifecycle_err}");
        }
        if let Some(ref cmd) = spec.on_done {
            let _ = spawn_on_done_command(cmd, &spec.task_id, "failed");
        }
    }
    Err(err)
}

async fn run_failed_post_lifecycle(store: &Arc<Store>, spec: &BackgroundRunSpec) -> Result<()> {
    let agent = lifecycle_agent_for_spec(store, spec)?;
    background_lifecycle::run_post_lifecycle(store, spec, &*agent, None).await
}

fn resolve_agent_for_spec(agent_name: &str) -> Result<Box<dyn agent::Agent>> {
    if let Some(kind) = AgentKind::parse_str(agent_name) {
        return Ok(agent::get_agent(kind));
    }
    if let Some(custom) = agent::registry::resolve_custom_agent(agent_name) {
        return Ok(custom);
    }
    anyhow::bail!("Unknown agent '{}'", agent_name)
}

fn lifecycle_agent_for_spec(
    store: &Arc<Store>,
    spec: &BackgroundRunSpec,
) -> Result<Box<dyn agent::Agent>> {
    if let Ok(agent) = resolve_agent_for_spec(&spec.agent_name) {
        return Ok(agent);
    }
    if let Some(task) = store.get_task(&spec.task_id)?
        && task.agent != AgentKind::Custom
    {
        return Ok(agent::get_agent(task.agent));
    }
    anyhow::bail!("Unknown agent '{}'", spec.agent_name)
}

async fn run_task_inner(store: &Arc<Store>, spec: &BackgroundRunSpec) -> Result<()> {
    let agent: Box<dyn agent::Agent> = resolve_agent_for_spec(&spec.agent_name)?;
    crate::rate_limit_wait::wait_for_declared_reset(
        store.as_ref(), &spec.task_id, agent.kind(), agent.rate_limit_name(),
    ).await?;
    let opts = RunOpts {
        dir: spec.dir.clone(),
        output: spec.output.clone(),
        result_file: spec.result_file.clone(),
        model: spec.model.clone(),
        budget: false,
        read_only: spec.read_only,
        sandbox: spec.sandbox,
        context_files: vec![],
        // Background runs intentionally stay fresh. If resume is added, carry the durable-home
        // context into command construction and record the same fallback milestone as foreground.
        session_id: None,
        env: agent::env_with_agent_log(
            spec.env.clone(),
            &spec.task_id,
            spec.container.is_none() && !spec.sandbox,
        ),
        env_forward: spec.env_forward.clone(),
    };
    ensure_agent_binary_available(spec)?;
    let _workspace_symlink = crate::cmd::run::WorkspaceSymlinkGuard::create(
        agent.kind(),
        spec.group.as_deref(),
        spec.dir.as_deref(),
    )?;
    let worktree_branch = store.get_task(&spec.task_id)?.and_then(|task| task.worktree_branch);
    let cargo_target_dir = agent::rust_build_cache_target_dir(spec.dir.as_deref(), worktree_branch.as_deref());
    let uses_durable_codex_home = agent::should_use_durable_codex_home(agent.kind(), spec.sandbox, spec.container.is_some());
    let mut std_cmd = agent
        .build_command_with_context(
            &spec.prompt,
            &opts,
            agent::CommandContext { durable_codex_home: uses_durable_codex_home, cargo_target_dir: cargo_target_dir.clone() },
        )
        .map_err(|err| anyhow::anyhow!("Failed to build agent command: {err:#}"))?;
    if spec.container.is_none() && !spec.sandbox {
        let program = std_cmd.get_program().to_string_lossy();
        agent::ensure_resolved_binary_available(&spec.agent_name, &program)?;
    }
    let _home_guard = agent::apply_run_env(&mut std_cmd, &opts, Some(&spec.task_id))?;
    if uses_durable_codex_home {
        agent::apply_codex_home_env(&mut std_cmd)?;
    }
    if let Some(ref dir) = spec.dir {
        agent::set_git_ceiling(&mut std_cmd, dir);
    }
    if let Some(ref group) = spec.group {
        std_cmd.env("AID_GROUP", group);
    }
    std_cmd.env("AID_TASK_ID", &spec.task_id);
    let depth = crate::cmd::run::task_depth(store, &spec.task_id).unwrap_or(0);
    std_cmd.env("AID_TASK_DEPTH", depth.to_string());
    agent::apply_cargo_target_env(&mut std_cmd, cargo_target_dir.as_deref());
    let container_name = if let Some(image) = spec.container.as_deref() {
        let project_dir = spec
            .dir
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or_else(|| std::path::Path::new("."));
        let project_id = crate::project::detect_project_in(project_dir)
            .map(|project| project.id)
            .unwrap_or_else(|| spec.task_id.clone());
        Some(crate::container::start_or_reuse(image, project_dir, &project_id)?)
    } else {
        None
    };
    let std_cmd = if let Some(container_name) = container_name.as_deref() {
        crate::container::exec_in_container(&std_cmd, container_name)
    } else if spec.sandbox && crate::sandbox::can_sandbox(agent.kind()) {
        if !crate::sandbox::is_available() {
            anyhow::bail!("--sandbox requires container CLI");
        }
        crate::sandbox::wrap_command(&std_cmd, &spec.task_id, agent.kind(), spec.read_only)
    } else {
        std_cmd
    };
    if spec.interactive {
        crate::pty_runner::run_agent_process_with_control(
            &*agent,
            &std_cmd,
            &TaskId(spec.task_id.clone()),
            store,
            &paths::log_path(&spec.task_id),
            spec.output.as_deref(),
            spec.model.as_deref(),
            agent.streaming(),
            crate::timeout_policy::TimeoutPolicy::from_env(spec.env.as_ref()),
            None,
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
            crate::timeout_policy::TimeoutPolicy::from_env(spec.env.as_ref()),
        )
        .await?;
    }
    if spec.sandbox {
        crate::sandbox::kill_container(&spec.task_id);
    }
    background_lifecycle::run_post_lifecycle(store, spec, &*agent, container_name.as_deref()).await?;
    Ok(())
}

fn ensure_agent_binary_available(spec: &BackgroundRunSpec) -> Result<()> {
    ensure_agent_binary_available_with(spec, agent::env::which_exists)
}

fn ensure_agent_binary_available_with<F>(spec: &BackgroundRunSpec, which: F) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    if spec.container.is_some() || spec.sandbox {
        return Ok(());
    }
    let Some(kind) = AgentKind::parse_str(&spec.agent_name) else {
        return Ok(());
    };
    agent::ensure_agent_binary_available_with(kind, &spec.agent_name, which)
}

#[cfg(test)]
#[path = "background_binary_tests.rs"]
mod background_binary_tests;
#[cfg(test)]
#[path = "background_foreground_tests.rs"]
mod foreground_tests;
#[cfg(test)]
#[path = "background_reaper_tests.rs"]
mod background_reaper_tests;
#[cfg(test)]
#[path = "background_lifecycle_bypass_tests.rs"]
mod lifecycle_bypass_tests;
#[cfg(test)]
mod tests;
