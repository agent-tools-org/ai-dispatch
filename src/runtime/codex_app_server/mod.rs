// Codex app-server execution selected once per dispatch, retaining exec as capability fallback.
// Shares normalized task events, completion accounting, and persisted steering requests.

mod command;
mod control;
mod home;
mod io;
mod turn;
pub(crate) use command::available;
pub(crate) use control::{active, receipt, wait_for_interrupt};

use crate::config::timeout_policy::TimeoutPolicy;
use crate::runtime::pty::runner_control::PtyRunControl;
use crate::{agent::Agent, store::Store, types::*};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub(crate) struct Run<'a> {
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

pub(crate) fn run(args: Run<'_>) -> Result<CompletionInfo> {
    let result = run_inner(&args);
    let info = match result {
        Ok(info) => info,
        Err(error) => {
            let detail = format!("Codex app-server: {error:#}");
            args.store.insert_agent_event(&TaskEvent {
                task_id: args.task.clone(),
                timestamp: chrono::Local::now(),
                event_kind: EventKind::Error,
                detail: detail.clone(),
                metadata: None,
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
                        Err(anyhow::anyhow!("{detail}")),
                    )?;
                }
            }
            CompletionInfo {
                status: TaskStatus::Failed,
                tokens: None,
                model: None,
                cost_usd: None,
                exit_code: None,
            }
        }
    };
    write_output(&args)?;
    Ok(info)
}

fn run_inner(args: &Run<'_>) -> Result<CompletionInfo> {
    let mut launch = command::translate(args.command)?;
    let task_dir = crate::support::paths::task_dir(args.task.as_str());
    std::fs::create_dir_all(&task_dir)?;
    let _home = if launch.isolated {
        let home = home::Home::for_command(args.command, &task_dir)?;
        launch.command.env("CODEX_HOME", &home.path);
        Some(home)
    } else {
        None
    };
    let rpc = args.store.with_running_task(args.task.as_str(), || {
        let rpc = io::Rpc::spawn(
            &mut launch.command,
            &crate::support::paths::stderr_path(args.task.as_str()),
        )?;
        let _ = crate::runtime::background::update_agent_pid(args.task.as_str(), rpc.pid());
        if let Some(control) = args.control {
            control.set_agent_pid(rpc.pid());
        }
        Ok(rpc)
    })?;
    let mut turn = turn::Turn::new(args, rpc)?;
    turn.start(launch)?;
    let active = control::Active::create(args.task.as_str(), turn.pid())?;
    let result = turn.monitor();
    drop(_home);
    drop(active);
    result
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
