// Detached background worker support for aid tasks.
// Persists run specs under ~/.aid/jobs and re-execs the binary to finish work.

#[path = "background_process.rs"]
mod background_process;
#[path = "background_launch.rs"]
mod background_launch;
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
#[path = "background_spawn.rs"]
mod background_spawn;

use anyhow::Result;
use std::process::Stdio;
use std::sync::Arc;

#[cfg(test)]
use self::background_process::build_on_done_command;
use self::background_process::spawn_on_done_command;
use self::background_reaper::record_worker_failure_skip_notify;
use self::background_spec::{load_spec, remove_spec};
use crate::agent;
use crate::cmd::run::RunArgs;
use crate::paths;
use crate::sanitize;
use crate::store::Store;
use crate::system_resources;
use crate::types::{AgentKind, TaskFilter, TaskId};

/// Hard limit on concurrent background workers — prevents process exhaustion.
const MAX_WORKERS: usize = 32;

pub use self::background_process::{is_process_running, kill_process, load_agent_pid, sigkill_process, update_agent_pid};
pub(crate) use self::background_spawn::{daemonize_worker_if_requested, spawn_worker};
pub use self::background_spec::{load_spec_if_exists, load_worker_pid, save_spec, BackgroundRunSpec};
pub(crate) use self::background_process::update_worker_pid;
pub(crate) use self::background_reaper::{check_zombie_tasks_with, record_failure};
#[cfg(test)]
pub(crate) use self::background_reaper::cleanup_stale_pending_tasks;
#[cfg(test)]
pub(crate) use self::background_reaper::{fail_stale_pending_task, ZOMBIE_FAILURE_DETAIL};
pub(crate) use self::background_spec::clear_spec;

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
    let args = RunArgs::for_worker(&store, &spec);
    let result = match args.as_ref() {
        Ok(args) => run_task_inner(&store, &spec, args).await,
        Err(err) => Err(anyhow::anyhow!("{err:#}")),
    };
    // Keep the completion barrier through error settlement as well.
    let result = match result {
        Ok(()) => Ok(()),
        Err(err) => handle_run_task_inner_error(&store, &spec, args.as_ref().ok(), err).await,
    };
    let _ = remove_spec(task_id);
    let _ = crate::input_signal::clear_response(task_id);
    let _ = crate::input_signal::clear_steer(task_id);
    result?;
    if let Some(cmd) = args.as_ref().ok().and_then(|args| args.on_done.as_ref()) {
        let _ = spawn_on_done_command(cmd, task_id, "done");
    }
    Ok(())
}

pub fn check_zombie_tasks(store: &Store) -> Result<Vec<String>> { check_zombie_tasks_with(store, is_process_running) }

async fn handle_run_task_inner_error(
    store: &Arc<Store>,
    spec: &BackgroundRunSpec,
    args: Option<&RunArgs>,
    err: anyhow::Error,
) -> Result<()> {
    let recorded_failure = record_worker_failure_skip_notify(store, &spec.task_id, &err)?;
    if !recorded_failure {
        return Err(err);
    }
    let Some(args) = args else {
        // Without saved args there is no lifecycle to notify on our behalf.
        if let Some(task) = store.get_task(&spec.task_id)? {
            crate::notify::notify_completion(&task);
        }
        return Err(err);
    };
    if let Err(lifecycle_err) = run_failed_post_lifecycle(store, spec, args).await {
        aid_error!("[aid] Background post-run lifecycle failed: {lifecycle_err}");
    }
    if let Some(ref cmd) = args.on_done {
        let _ = spawn_on_done_command(cmd, &spec.task_id, "failed");
    }
    Err(err)
}

async fn run_failed_post_lifecycle(store: &Arc<Store>, spec: &BackgroundRunSpec, args: &RunArgs) -> Result<()> {
    let agent = lifecycle_agent_for_task(store, spec, args)?;
    background_lifecycle::run_post_lifecycle(store, spec, args, &*agent, None).await
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

fn lifecycle_agent_for_task(
    store: &Arc<Store>,
    spec: &BackgroundRunSpec,
    args: &RunArgs,
) -> Result<Box<dyn agent::Agent>> {
    if let Ok(agent) = resolve_agent_for_spec(&args.agent_name) {
        return Ok(agent);
    }
    if let Some(task) = store.get_task(&spec.task_id)?
        && task.agent != AgentKind::Custom
    {
        return Ok(agent::get_agent(task.agent));
    }
    anyhow::bail!("Unknown agent '{}'", args.agent_name)
}

async fn run_task_inner(store: &Arc<Store>, spec: &BackgroundRunSpec, args: &RunArgs) -> Result<()> {
    let agent = resolve_agent_for_spec(&args.agent_name)?;
    crate::rate_limit_wait::wait_for_declared_reset(
        store.as_ref(), &spec.task_id, agent.kind(), agent.rate_limit_name(),
    ).await?;
    ensure_agent_binary_available(args)?;
    let _workspace_symlink = crate::cmd::run::WorkspaceSymlinkGuard::create(
        agent.kind(), args.group.as_deref(), args.dir.as_deref(),
    )?;
    let home_guard = agent::home_isolation::IsolatedHomeGuard::create_with_remote_build(
        Some(&spec.task_id), args.remote_build.is_some(),
    )?;
    let (std_cmd, container_name) = background_launch::prepare_launch(
        store, spec, args, agent.as_ref(), &home_guard,
    )?;
    run_process(store, spec, args, agent.as_ref(), std_cmd).await?;
    if args.sandbox {
        crate::sandbox::kill_container(&spec.task_id);
    }
    background_lifecycle::run_post_lifecycle(store, spec, args, &*agent, container_name.as_deref()).await?;
    Ok(())
}

async fn run_process(
    store: &Arc<Store>, spec: &BackgroundRunSpec, args: &RunArgs,
    agent: &dyn agent::Agent, std_cmd: std::process::Command,
) -> Result<()> {
    let task_id = TaskId(spec.task_id.clone());
    let log_path = paths::log_path(&spec.task_id);
    let timeout_policy = crate::timeout_policy::TimeoutPolicy::from_env(args.env.as_ref());
    if spec.interactive {
        crate::pty_runner::run_agent_process_with_control(
            agent, &std_cmd, &task_id, store, &log_path,
            args.output.as_deref(), args.model.as_deref(), agent.streaming(),
            timeout_policy, args.max_task_cost, None,
        )?;
        return Ok(());
    }
    let mut tokio_cmd = tokio::process::Command::from(std_cmd);
    tokio_cmd.stdout(Stdio::piped());
    tokio_cmd.stderr(Stdio::piped());
    crate::cmd::run::run_agent_process_with_cost(
        agent, tokio_cmd, &task_id, store, &log_path,
        args.output.as_deref(), args.model.as_deref(), agent.streaming(),
        args.group.as_deref(), timeout_policy, args.max_task_cost,
    )
    .await?;
    Ok(())
}

fn ensure_agent_binary_available(args: &RunArgs) -> Result<()> {
    ensure_agent_binary_available_with(args, agent::env::which_exists)
}

fn ensure_agent_binary_available_with<F>(args: &RunArgs, which: F) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    if args.container.is_some() || args.sandbox {
        return Ok(());
    }
    let Some(kind) = AgentKind::parse_str(&args.agent_name) else {
        return Ok(());
    };
    agent::ensure_agent_binary_available_with(kind, &args.agent_name, which)
}

#[cfg(test)]
#[path = "background_binary_tests.rs"]
mod background_binary_tests;
#[cfg(test)]
#[path = "background_reaper_tests.rs"]
mod background_reaper_tests;
#[cfg(test)]
#[path = "background_lifecycle_bypass_tests.rs"]
mod lifecycle_bypass_tests;
#[cfg(test)]
mod tests;
