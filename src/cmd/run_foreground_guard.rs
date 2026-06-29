// Foreground run guard for in-process `aid run` execution.
// Exports spec persistence, signal waiting, and interruption cleanup helpers.
// Deps: background specs/process cleanup, run dispatch types, store events.
use anyhow::{Context, Result};
use chrono::Local;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;

use crate::background::{self, BackgroundRunSpec};
use crate::store::Store;
use crate::types::{AgentKind, EventKind, TaskEvent, TaskId};

use super::run_dispatch_prepare::PreparedDispatch;
use super::run_agent::run_agent_process_with_timeout;
use super::{RunArgs, run_prompt};

pub(super) struct ForegroundSpecGuard {
    task_id: String,
    active: bool,
}

impl ForegroundSpecGuard {
    pub(super) fn save(spec: BackgroundRunSpec) -> Result<Self> {
        let task_id = spec.task_id.clone();
        background::save_spec(&spec)?;
        Ok(Self { task_id, active: true })
    }
    pub(super) fn clear_now(&mut self) -> Result<()> {
        if self.active {
            background::clear_spec(&self.task_id)?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for ForegroundSpecGuard { fn drop(&mut self) { let _ = self.clear_now(); } }

pub(super) fn save_foreground_spec(
    args: &RunArgs,
    prepared: &PreparedDispatch,
    prompt_bundle: &run_prompt::PromptBundle,
    pre_task_dirty_paths: Option<Vec<String>>,
) -> Result<ForegroundSpecGuard> {
    ForegroundSpecGuard::save(BackgroundRunSpec {
        task_id: prepared.task_id.as_str().to_string(),
        worker_pid: Some(std::process::id()),
        agent_name: prepared.agent_display_name.clone(),
        prompt: prompt_bundle.effective_prompt.clone(),
        dir: prepared.effective_dir.clone(),
        output: args.output.clone(),
        result_file: args.result_file.clone(),
        model: prepared.effective_model.clone(),
        verify: args.verify.clone(),
        setup: args.setup.clone(),
        iterate: args.iterate,
        eval: args.eval.clone(),
        eval_feedback_template: args.eval_feedback_template.clone(),
        judge: args.judge.clone(),
        max_duration_mins: args.max_duration_mins,
        idle_timeout_secs: crate::idle_timeout::idle_timeout_secs_from_env(args.env.as_ref()),
        retry: args.retry,
        group: args.group.clone(),
        skills: args.skills.clone(),
        checklist: args.checklist.clone(),
        template: args.template.clone(),
        interactive: true,
        on_done: args.on_done.clone(),
        cascade: args.cascade.clone(),
        parent_task_id: args.parent_task_id.clone(),
        env: args.env.clone(),
        env_forward: args.env_forward.clone(),
        agent_pid: None,
        sandbox: args.sandbox,
        read_only: args.read_only,
        audit_report_mode: args.audit_report_mode,
        container: args.container.clone(),
        link_deps: args.link_deps,
        pre_task_dirty_paths,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ForegroundSignal {
    Int,
    Term,
    Hup,
}

impl ForegroundSignal {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Int => "SIGINT",
            Self::Term => "SIGTERM",
            Self::Hup => "SIGHUP",
        }
    }
}

// SIGKILL cannot be caught; the foreground spec remains the convergence backstop for that case.
#[cfg(unix)]
pub(super) async fn wait_for_foreground_signal() -> Result<ForegroundSignal> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sighup = signal(SignalKind::hangup())?;
    tokio::select! {
        _ = sigint.recv() => Ok(ForegroundSignal::Int),
        _ = sigterm.recv() => Ok(ForegroundSignal::Term),
        _ = sighup.recv() => Ok(ForegroundSignal::Hup),
    }
}

#[cfg(not(unix))]
pub(super) async fn wait_for_foreground_signal() -> Result<ForegroundSignal> {
    std::future::pending::<Result<ForegroundSignal>>().await
}

pub(super) fn handle_foreground_interrupt(store: &Store, task_id: &TaskId, signal_name: &str) -> Result<()> {
    handle_foreground_interrupt_with(store, task_id, signal_name, background::kill_process, None)
}

fn handle_foreground_interrupt_with<F>(
    store: &Store,
    task_id: &TaskId,
    signal_name: &str,
    mut kill_agent: F,
    fallback_agent_pid: Option<u32>,
) -> Result<()>
where
    F: FnMut(u32),
{
    record_interrupted(store, task_id, signal_name)?;
    if let Some(agent_pid) = background::load_agent_pid(task_id.as_str())?.or(fallback_agent_pid) {
        kill_agent(agent_pid);
    }
    background::clear_spec(task_id.as_str())?;
    Ok(())
}

fn record_interrupted(store: &Store, task_id: &TaskId, signal_name: &str) -> Result<()> {
    let detail = format!("interrupted by signal {signal_name}");
    if !store.fail_if_running(task_id.as_str())? {
        return Ok(());
    }
    std::fs::write(crate::paths::stderr_path(task_id.as_str()), format!("{detail}\n"))?;
    crate::pty_watch::append_failed_terminal_sentinel(task_id, &crate::paths::log_path(task_id.as_str()), &detail);
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata: None,
    })?;
    run_prompt::notify_task_completion(store, task_id)?;
    Ok(())
}

pub(super) fn foreground_agent(agent_kind: AgentKind, agent_display_name: &str) -> Result<Box<dyn crate::agent::Agent>> {
    if agent_kind == AgentKind::Custom {
        return crate::agent::registry::resolve_custom_agent(agent_display_name)
            .ok_or_else(|| anyhow::anyhow!("Custom agent '{}' not found in registry", agent_display_name));
    }
    Ok(crate::agent::get_agent(agent_kind))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_agent_with_signal(
    agent: &dyn crate::agent::Agent, agent_display_name: &str,
    std_cmd: std::process::Command,
    task_id: &TaskId, store: &Arc<Store>, log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    workgroup_id: Option<&str>,
    max_duration_mins: Option<i64>,
    max_task_cost: Option<f64>,
) -> Result<()> {
    if agent.needs_pty() {
        return run_pty_agent_with_signal(
            agent.kind(),
            agent_display_name,
            std_cmd,
            task_id,
            store,
            log_path,
            output_path,
            model,
            agent.streaming(),
        )
        .await;
    }
    run_non_pty_agent_with_signal(
        agent,
        std_cmd,
        task_id,
        store,
        log_path,
        output_path,
        model,
        workgroup_id,
        max_duration_mins,
        max_task_cost,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_pty_agent_with_signal(
    agent_kind: AgentKind, agent_display_name: &str,
    std_cmd: std::process::Command,
    task_id: &TaskId, store: &Arc<Store>, log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
) -> Result<()> {
    let task_id_for_run = task_id.clone();
    let task_id_for_signal = task_id.clone();
    let store_for_run = Arc::clone(store);
    let store_for_signal = Arc::clone(store);
    let log_path = PathBuf::from(log_path);
    let output_path = output_path.map(ToOwned::to_owned);
    let model = model.map(ToOwned::to_owned);
    let agent_display_name = agent_display_name.to_string();
    let control = crate::pty_runner_control::PtyRunControl::default();
    let control_for_run = control.clone();
    let mut run_handle = tokio::task::spawn_blocking(move || {
        let agent = foreground_agent(agent_kind, &agent_display_name)?;
        crate::pty_runner::run_agent_process_with_control(
            &*agent,
            &std_cmd,
            &task_id_for_run,
            &store_for_run,
            &log_path,
            output_path.as_deref(),
            model.as_deref(),
            streaming,
            Some(control_for_run),
        )
    });
    tokio::select! {
        result = &mut run_handle => result.context("foreground PTY runner panicked")?,
        signal = wait_for_foreground_signal() => {
            let signal = signal?;
            control.mark_interrupted();
            let fallback_pid = control.wait_agent_pid(std::time::Duration::from_secs(2)).await;
            handle_foreground_interrupt_with(
                &store_for_signal,
                &task_id_for_signal,
                signal.name(),
                background::kill_process,
                fallback_pid,
            )?;
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), &mut run_handle).await;
            anyhow::bail!("interrupted by signal {}", signal.name());
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_non_pty_agent_with_signal(
    agent: &dyn crate::agent::Agent, std_cmd: std::process::Command,
    task_id: &TaskId, store: &Arc<Store>, log_path: &Path,
    output_path: Option<&str>,
    model: Option<&str>,
    workgroup_id: Option<&str>,
    max_duration_mins: Option<i64>,
    max_task_cost: Option<f64>,
) -> Result<()> {
    let mut tokio_cmd = Command::from(std_cmd);
    tokio_cmd.stdout(std::process::Stdio::piped());
    tokio_cmd.stderr(std::process::Stdio::piped());
    let run = run_agent_process_with_timeout(
        agent,
        tokio_cmd,
        task_id,
        store,
        log_path,
        output_path,
        model,
        agent.streaming(),
        workgroup_id,
        max_duration_mins,
        max_task_cost,
    );
    tokio::pin!(run);
    tokio::select! {
        result = &mut run => result,
        signal = wait_for_foreground_signal() => {
            let signal = signal?;
            handle_foreground_interrupt(store, task_id, signal.name())?;
            anyhow::bail!("interrupted by signal {}", signal.name());
        }
    }
}

#[cfg(test)]
#[path = "run_foreground_guard_tests.rs"]
mod tests;
