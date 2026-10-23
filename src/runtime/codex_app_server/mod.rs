// Codex app-server execution selected once per dispatch, retaining exec as capability fallback.
// Shares normalized task events, completion accounting, and persisted steering requests.

pub(crate) mod command;
mod control;
mod home;
mod io;
mod resume;
mod turn;
#[cfg(test)]
#[path = "turn/fixture.rs"]
mod fixture;
pub(crate) use command::available;
pub(crate) use control::{active, receipt, wait_for_interrupt};
pub(crate) use resume::exec_fallback;

use crate::config::timeout_policy::TimeoutPolicy;
use crate::runtime::pty::runner_control::PtyRunControl;
use crate::{agent::Agent, store::Store, types::*};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub(crate) struct Run<'a> {
    pub start: std::time::Instant,
    pub agent: &'a dyn Agent,
    pub command: &'a std::process::Command,
    pub task: &'a TaskId,
    pub store: &'a Arc<Store>,
    pub log: &'a Path,
    pub output: Option<&'a str>,
    pub group: Option<&'a str>,
    pub policy: TimeoutPolicy,
    pub max_cost: Option<f64>,
    pub control: Option<&'a PtyRunControl>,
}

pub(crate) fn run(args: Run<'_>) -> Result<Option<CompletionInfo>> {
    let started = args.start;
    let info = match run_inner(&args) {
        Ok(info) => info,
        Err(error) => {
            if started.elapsed() >= args.policy.max_duration.min(args.policy.hard_cap)
                || args
                    .store
                    .get_task(args.task.as_str())?
                    .is_none_or(|task| task.status.is_terminal())
            {
                Some(failed(&args, &error.to_string())?)
            } else {
                fallback(&args, &format!("{error:#}"))?;
                None
            }
        }
    };
    if info.is_some() {
        write_output(&args)?;
    }
    Ok(info)
}

fn fallback(args: &Run<'_>, reason: &str) -> Result<()> {
    let stderr =
        crate::runtime::watcher::read_capped_stderr(args.task.as_str()).unwrap_or_default();
    args.store.insert_event(&TaskEvent {
        task_id: args.task.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: format!(
            "Codex app-server startup failed; falling back to exec{}: {reason}",
            if reason.contains("Codex thread/resume:") {
                " with a fresh session"
            } else {
                ""
            }
        ),
        metadata: Some(
            serde_json::json!({"codex_app_server_fallback":true,"reason":reason,"stderr":stderr}),
        ),
    })?;
    Ok(())
}

fn failed(args: &Run<'_>, reason: &str) -> Result<CompletionInfo> {
    let stderr =
        crate::runtime::watcher::read_capped_stderr(args.task.as_str()).unwrap_or_default();
    let message = format!("Codex app-server: {reason}\n{stderr}");
    let (detail, metadata) = crate::agent::truncate::capped_detail(&message);
    use std::io::Write;
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(args.log)?;
    writeln!(
        log,
        "{}",
        serde_json::json!({"type":"error","message":message})
    )?;
    args.store.insert_agent_event(&TaskEvent {
        task_id: args.task.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Error,
        detail,
        metadata,
    })?;
    for message in args.store.pending_inbound_for_task(args.task.as_str())? {
        if matches!(
            receipt(args.store, args.task.as_str(), message.id),
            Ok(None)
        ) {
            control::record(
                args.store,
                args.task,
                message.id,
                Err(anyhow::anyhow!("{reason}")),
            )?;
        }
    }
    Ok(CompletionInfo {
        status: TaskStatus::Failed,
        tokens: None,
        model: None,
        cost_usd: None,
        exit_code: None,
    })
}

fn run_inner(args: &Run<'_>) -> Result<Option<CompletionInfo>> {
    let mut launch = prepare(args)?;
    let rpc = spawn(args, &mut launch)?;
    let active = control::Active::starting(args.task.as_str(), rpc.pid())?;
    let mut turn = turn::Turn::new(args, rpc)?;
    if let Err(error) = turn.start(launch) {
        if turn.fallback_safe() {
            return Err(error);
        }
        return failed(args, &error.to_string()).map(Some);
    }
    if let Err(error) = active.ready() {
        return failed(args, &error.to_string()).map(Some);
    }
    let result = match turn.monitor() {
        Ok(info) => Ok(Some(info)),
        Err(error) => failed(args, &error.to_string()).map(Some),
    };
    drop(active);
    result
}

fn prepare(args: &Run<'_>) -> Result<command::Launch> {
    let mut launch = command::translate(args.command)?;
    std::fs::create_dir_all(crate::support::paths::task_dir(args.task.as_str()))?;
    if launch.isolated {
        let home = home::Home::for_command(args.command)?;
        launch.command.env("CODEX_HOME", home.path);
    }
    Ok(launch)
}

fn spawn(args: &Run<'_>, launch: &mut command::Launch) -> Result<io::Rpc> {
    io::Rpc::prepare(
        &mut launch.command,
        &crate::support::paths::stderr_path(args.task.as_str()),
    )?;
    let child = args.store.with_running_task(args.task.as_str(), || {
        io::Rpc::spawn(&mut launch.command)
    })?;
    let rpc = io::Rpc::attach(child)?;
    args.store.publish_launch(args.task.as_str(), rpc.pid(), || {
        crate::runtime::background::update_agent_pid(args.task.as_str(), rpc.pid())
    })?;
    if let Some(control) = args.control {
        control.set_agent_pid(rpc.pid());
    }
    Ok(rpc)
}

fn write_output(args: &Run<'_>) -> Result<()> {
    let log = std::fs::read_to_string(args.log).unwrap_or_default();
    let transcript = crate::support::paths::transcript_path(args.task.as_str());
    if let Some(parent) = transcript.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(transcript, &log)?;
    if let Some(path) = args.output {
        let text: Vec<_> = log
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| {
                value["type"] == "item.completed" && value["item"]["type"] == "agent_message"
            })
            .filter_map(|value| value["item"]["text"].as_str().map(str::to_string))
            .collect();
        if let Some(final_message) = text.last() {
            std::fs::write(path, final_message)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "read_only_transport_tests.rs"]
mod read_only_transport_tests;
