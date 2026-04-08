// Detached background worker support for aid tasks.
// Persists run specs under ~/.aid/jobs and re-execs the binary to finish work.

#[path = "background_process.rs"]
mod background_process;
#[path = "background_spec.rs"]
mod background_spec;
#[path = "background_waiting.rs"]
mod background_waiting;

use anyhow::{Context, Result};
use chrono::Local;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

#[cfg(test)]
use self::background_process::build_on_done_command;
use self::background_process::spawn_on_done_command;
use self::background_spec::{load_spec, load_spec_if_exists, remove_spec};
use crate::agent::{self, RunOpts};
use crate::config;
use crate::notify;
use crate::paths;
use crate::sanitize;
use crate::store::Store;
use crate::system_resources;
use crate::types::{AgentKind, EventKind, PendingReason, Task, TaskEvent, TaskFilter, TaskId, TaskStatus};

const ZOMBIE_FAILURE_DETAIL: &str = "Background worker died unexpectedly";
const PENDING_TASK_TIMEOUT_SECS: i64 = 600;
const MAX_RUN_HOURS: i64 = 24;
/// Hard limit on concurrent background workers — prevents process exhaustion.
const MAX_WORKERS: usize = 32;

pub use self::background_process::{is_process_running, kill_process, load_agent_pid, sigkill_process, update_agent_pid};
pub use self::background_spec::{load_worker_pid, save_spec, BackgroundRunSpec};
pub(crate) use self::background_process::update_worker_pid;
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

pub fn check_zombie_tasks(store: &Store) -> Result<Vec<String>> { check_zombie_tasks_with(store, is_process_running) }

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
        result_file: spec.result_file.clone(),
        model: spec.model.clone(),
        budget: false,
        read_only: spec.read_only,
        context_files: vec![],
        session_id: None,
        env: spec.env.clone(),
        env_forward: spec.env_forward.clone(),
    };
    let _workspace_symlink = crate::cmd::run::WorkspaceSymlinkGuard::create(
        agent.kind(),
        spec.group.as_deref(),
        spec.dir.as_deref(),
    )?;
    let mut std_cmd = agent
        .build_command(&spec.prompt, &opts)
        .context("Failed to build agent command")?;
    agent::apply_run_env(&mut std_cmd, &opts);
    if let Some(ref dir) = spec.dir {
        agent::set_git_ceiling(&mut std_cmd, dir);
    }
    if let Some(ref group) = spec.group {
        std_cmd.env("AID_GROUP", group);
    }
    std_cmd.env("AID_TASK_ID", &spec.task_id);
    let worktree_branch = store.get_task(&spec.task_id)?.and_then(|task| task.worktree_branch);
    if agent::is_rust_project(spec.dir.as_deref())
        && let Some(target_dir) =
            agent::target_dir_for_worktree(worktree_branch.as_deref())
    {
        std_cmd.env("CARGO_TARGET_DIR", &target_dir);
    }
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
    if spec.sandbox {
        crate::sandbox::kill_container(&spec.task_id);
    }
    let retry_args = crate::cmd::run::RunArgs {
        agent_name: spec.agent_name.clone(),
        prompt: spec.prompt.clone(),
        dir: spec.dir.clone(),
        output: spec.output.clone(),
        result_file: spec.result_file.clone(),
        model: spec.model.clone(),
        group: spec.group.clone(),
        verify: spec.verify.clone(),
        iterate: spec.iterate,
        eval: spec.eval.clone(),
        eval_feedback_template: spec.eval_feedback_template.clone(),
        judge: spec.judge.clone(),
        max_duration_mins: spec.max_duration_mins,
        retry: spec.retry,
        checklist: spec.checklist.clone(),
        skills: spec.skills.clone(),
        template: spec.template.clone(),
        cascade: spec.cascade.clone(),
        parent_task_id: spec.parent_task_id.clone(),
        env: spec.env.clone(),
        env_forward: spec.env_forward.clone(),
        read_only: spec.read_only,
        sandbox: spec.sandbox,
        container: spec.container.clone(),
        ..Default::default()
    };
    // Auto-commit + rescue untracked files BEFORE verify — ensures verify tests committed state
    let is_read_only = store.get_task(&spec.task_id)?.map(|t| t.read_only).unwrap_or(false);
    if !is_read_only
        && let Some(worktree_dir) = spec.dir.as_deref()
        && crate::commit::has_uncommitted_changes(worktree_dir).unwrap_or(false)
        && let Err(e) = crate::commit::auto_commit(worktree_dir, &spec.task_id, &spec.prompt)
    {
        aid_error!("[aid] auto-commit failed: {e}");
        let _ = store.insert_event(&TaskEvent {
            task_id: TaskId(spec.task_id.clone()),
            timestamp: Local::now(),
            event_kind: EventKind::Error,
            detail: format!("Auto-commit failed: {e}"),
            metadata: None,
        });
    }
    if !is_read_only {
        if let Some(worktree_dir) = spec.dir.as_deref() {
            match crate::commit::rescue_untracked_files(worktree_dir, &spec.task_id) {
                Ok(rescued) if !rescued.is_empty() => {
                    let files_list = rescued.join(", ");
                    aid_warn!("[aid] Rescued {} untracked file(s) into commit: {}", rescued.len(), files_list);
                }
                Err(e) => aid_warn!("[aid] Untracked file rescue failed: {e}"),
                _ => {}
            }
        }
    }
    let pre_verify_status = store.get_task(&spec.task_id)?.map(|task| task.status).unwrap_or(TaskStatus::Done);
    crate::cmd::run::maybe_verify(store, &TaskId(spec.task_id.clone()), spec.verify.as_deref(), spec.dir.as_deref(), container_name.as_deref());
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
    if let Err(err) = crate::cmd::run::persist_result_file(&spec.task_id, spec.result_file.as_deref(), spec.dir.as_deref()) {
        aid_warn!("[aid] Failed to persist result file: {err}");
    }
    let iterate_config = crate::cmd::run::iterate_config(&retry_args)?;
    if let Some(iterate_config) = iterate_config.as_ref()
        && crate::cmd::run::maybe_iterate(
            store,
            &TaskId(spec.task_id.clone()),
            &retry_args,
            iterate_config,
        )
        .await?
        .is_some()
    {
        return Ok(());
    }
    if iterate_config.is_none()
        && crate::cmd::run::maybe_auto_retry_after_verify_failure(
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
    crate::verify::enforce_verify_status(store, &TaskId(spec.task_id.clone()));
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
    } else if spec.group.is_none()
        && let Some(task) = store.get_task(&spec.task_id)?
        && task.status == TaskStatus::Failed
    {
        if !spec.cascade.is_empty() {
            // Explicit --cascade list: try next agent in the chain
            let next_agent = spec.cascade[0].clone();
            let remaining: Vec<String> = spec.cascade[1..].to_vec();
            aid_info!(
                "[aid] Cascade: trying {} after {} failed",
                next_agent,
                spec.agent_name
            );
            let mut cascade_args = retry_args;
            cascade_args.agent_name = next_agent;
            cascade_args.cascade = remaining;
            cascade_args.parent_task_id = Some(spec.task_id.clone());
            Box::pin(crate::cmd::run::run(store.clone(), cascade_args)).await?;
        } else {
            // Quota auto-cascade: only when no explicit cascade (batch_dispatch handles its own fallback)
            let agent_kind = AgentKind::parse_str(&spec.agent_name);
            if let Some(kind) = agent_kind
                && let Some(message) = crate::cmd::run::read_quota_error_message(&TaskId(spec.task_id.clone()))
            {
                crate::rate_limit::mark_rate_limited(&kind, &message);
                crate::cmd::run::rescue_quota_failed_task(
                    store,
                    &TaskId(spec.task_id.clone()),
                    Some(&message),
                );
                if let Some(fallback) = agent::selection::coding_fallback_for(&kind) {
                    let rescued = store
                        .get_task(&spec.task_id)?
                        .map(|task| task.status == TaskStatus::Done)
                        .unwrap_or(false);
                    if !rescued {
                        aid_info!(
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
        }
    }

    Ok(())
}

fn record_worker_failure(store: &Store, task_id: &str, err: &anyhow::Error) -> Result<()> { record_failure(store, task_id, &format!("{err:#}"), &format!("Background worker failed: {err}")) }

fn check_zombie_tasks_with<F>(store: &Store, is_worker_alive: F) -> Result<Vec<String>>
where
    F: Fn(u32) -> bool,
{
    let config = config::load_config()?;
    let mut cleaned = cleanup_stale_pending_tasks(store)?;
    cleaned.extend(background_waiting::cleanup_stale_waiting_tasks(
        store,
        config.background.max_task_duration_mins,
    )?);
    let running_tasks = store.list_tasks(TaskFilter::Running)?;
    for task in &running_tasks {
        let task_id = task.id.as_str();
        let Some(spec) = load_spec_if_exists(task_id)? else {
            continue;
        };
        let Some(worker_pid) = spec.worker_pid else {
            // Grace period: tasks without worker_pid may be in the dispatch window
            // (spec created but worker not yet spawned). Skip if created < 60s ago.
            let age_secs = (Local::now() - task.created_at).num_seconds();
            if age_secs < 60 {
                continue;
            }
            if let Some(task) = store.get_task(task_id)?
                && !task.read_only
                && let Some(ref path) = task.worktree_path
                && std::path::Path::new(path).exists()
                && crate::commit::has_uncommitted_changes(path).unwrap_or(false)
            {
                let _ = crate::commit::auto_commit(path, task_id, &task.prompt);
                aid_info!("[aid] Preserved uncommitted changes for zombie task {task_id}");
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
            && !task.read_only
            && let Some(ref path) = task.worktree_path
            && std::path::Path::new(path).exists()
            && crate::commit::has_uncommitted_changes(path).unwrap_or(false)
        {
            let _ = crate::commit::auto_commit(path, task_id, &task.prompt);
            aid_info!("[aid] Preserved uncommitted changes for zombie task {task_id}");
        }
        record_failure(store, task_id, ZOMBIE_FAILURE_DETAIL, ZOMBIE_FAILURE_DETAIL)?;
        if let Some(agent_pid) = spec.agent_pid {
            kill_process(agent_pid);
        }
        cleaned.push(task_id.to_string());
    }
    for task in running_tasks {
        let task_id = task.id.as_str();
        if cleaned.iter().any(|id| id == task_id) {
            continue;
        }
        let elapsed = (Local::now() - task.created_at).num_hours();
        if elapsed <= MAX_RUN_HOURS {
            continue;
        }
        aid_info!(
            "[aid] Auto-failing stale task {} (running {}h, max {}h)",
            task.id,
            elapsed,
            MAX_RUN_HOURS
        );
        record_failure(
            store,
            task_id,
            "Auto-failed: exceeded 24h maximum runtime",
            "Task exceeded maximum runtime (24h)",
        )?;
        cleaned.push(task_id.to_string());
    }
    Ok(cleaned)
}

fn cleanup_stale_pending_tasks(store: &Store) -> Result<Vec<String>> {
    let now = Local::now();
    let mut cleaned = Vec::new();
    for task in store.list_tasks(TaskFilter::All)? {
        if task.status != TaskStatus::Pending {
            continue;
        }
        let elapsed_secs = (now - task.created_at).num_seconds();
        if elapsed_secs <= PENDING_TASK_TIMEOUT_SECS {
            continue;
        }
        let task_id = task.id.as_str();
        aid_warn!("[aid] Timing out stale pending task {} (pending for {}s)", task_id, elapsed_secs);
        if !fail_stale_pending_task(store, &task, now, elapsed_secs)? {
            continue;
        }
        cleaned.push(task_id.to_string());
    }
    Ok(cleaned)
}

fn fail_stale_pending_task(
    store: &Store,
    task: &Task,
    now: chrono::DateTime<Local>,
    elapsed_secs: i64,
) -> Result<bool> {
    let task_id = task.id.as_str();
    let pending_reason = infer_pending_reason(store, task)?;
    if !store.fail_pending_with_reason(task_id, pending_reason)? {
        return Ok(false);
    }
    store.insert_event(&TaskEvent {
        task_id: task.id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: format!(
            "Task timed out in pending state after {}s (reason: {})",
            elapsed_secs,
            pending_reason.as_str()
        ),
        metadata: None,
    })?;
    notify_task_completion(store, task_id)?;
    Ok(true)
}

fn infer_pending_reason(store: &Store, task: &Task) -> Result<PendingReason> {
    if task.agent != AgentKind::Custom && crate::rate_limit::is_rate_limited(&task.agent) {
        return Ok(PendingReason::RateLimited);
    }
    if store.list_tasks(TaskFilter::Running)?.len() >= MAX_WORKERS {
        return Ok(PendingReason::WorkerCapacity);
    }
    let has_agent_pid = load_spec_if_exists(task.id.as_str())?
        .and_then(|spec| spec.agent_pid)
        .is_some();
    if has_agent_pid {
        Ok(PendingReason::AgentStarting)
    } else {
        Ok(PendingReason::Unknown)
    }
}

fn record_failure(store: &Store, task_id: &str, stderr_detail: &str, event_detail: &str) -> Result<()> {
    sanitize::validate_task_id(task_id)?;
    // Only mark as failed if task is still running/waiting — prevents
    // zombie cleanup from clobbering a real completion status.
    if !store.fail_if_running(task_id)? {
        return Ok(());
    }
    let stderr_path = paths::stderr_path(task_id);
    std::fs::write(&stderr_path, format!("{stderr_detail}\n"))?;
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
    if let Some(task) = store.get_task(task_id)? { notify::notify_completion(&task); }
    Ok(())
}

#[cfg(test)]
mod tests;
