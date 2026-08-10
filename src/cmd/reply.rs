// Handler for `aid reply` task message delivery and ack polling.
// Exports: run plus the reply outcome used by CLI dispatch.
// Deps: crate::store::Store, crate::input_signal, crate::types.

use std::io::Read;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

use crate::agent;
use crate::input_signal;
use crate::store::Store;
use crate::types::{MessageDirection, MessageSource, Task, TaskStatus};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyOutcome {
    Queued { id: i64 },
    Acked { delivered: bool },
    TimedOut { delivered: bool },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum InputCommand {
    Reply,
    Steer,
    Respond,
    Unstick,
}

impl InputCommand {
    fn invocation(self) -> &'static str {
        match self {
            Self::Reply => "`aid reply`",
            Self::Steer => "`aid steer`",
            Self::Respond => "`aid respond`",
            Self::Unstick => "`aid unstick`",
        }
    }

    fn refusal(self) -> &'static str {
        match self {
            Self::Reply => "no reply message was queued",
            Self::Steer => "no steer message was queued",
            Self::Respond => "no response signal was written",
            Self::Unstick => "no nudge was queued",
        }
    }
}

pub fn run(
    store: &Store,
    task_id: &str,
    message: Option<&str>,
    file: Option<&str>,
    async_mode: bool,
    timeout_secs: u64,
) -> Result<ReplyOutcome> {
    run_with_source(
        store,
        task_id,
        message,
        file,
        async_mode,
        timeout_secs,
        MessageSource::Reply,
        InputCommand::Reply,
    )
}

pub(crate) fn run_with_source(
    store: &Store,
    task_id: &str,
    message: Option<&str>,
    file: Option<&str>,
    async_mode: bool,
    timeout_secs: u64,
    source: MessageSource,
    command: InputCommand,
) -> Result<ReplyOutcome> {
    run_with_hook(
        store,
        task_id,
        message,
        file,
        async_mode,
        Duration::from_secs(timeout_secs),
        DEFAULT_POLL_INTERVAL,
        source,
        command,
        |_| {},
    )
}

fn run_with_hook<F>(
    store: &Store,
    task_id: &str,
    message: Option<&str>,
    file: Option<&str>,
    async_mode: bool,
    timeout: Duration,
    poll_interval: Duration,
    source: MessageSource,
    command: InputCommand,
    mut on_poll: F,
) -> Result<ReplyOutcome>
where
    F: FnMut(i64),
{
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task {task_id} not found"))?;
    if !matches!(
        task.status,
        TaskStatus::Running | TaskStatus::AwaitingInput | TaskStatus::Stalled
    ) {
        bail!(
            "Task {task_id} is {} — can only reply to running tasks",
            task.status.label()
        );
    }
    ensure_interactive_input(&task, command)?;

    let text = read_message(message, file)?;
    let queued = store.insert_message(task_id, MessageDirection::In, &text, source)?;
    input_signal::write_steer(task_id, &text)?;
    if async_mode {
        return Ok(ReplyOutcome::Queued { id: queued.id });
    }

    wait_for_ack(store, task_id, queued.id, timeout, poll_interval, &mut on_poll)
}

pub(crate) fn ensure_interactive_input(task: &Task, command: InputCommand) -> Result<()> {
    let adapter = if task.agent == crate::types::AgentKind::Custom {
        task.custom_agent_name
            .as_deref()
            .and_then(agent::registry::resolve_custom_agent)
            .ok_or_else(|| {
                let name = task.custom_agent_name.as_deref().unwrap_or("<unnamed>");
                anyhow!(
                    "Task {} uses unavailable custom agent '{}'; restore ~/.aid/agents/{}.toml, or stop the task and retry it with an available agent",
                    task.id, name, name
                )
            })?
    } else {
        agent::get_agent(task.agent)
    };
    if adapter.accepts_interactive_input() {
        return Ok(());
    }
    let agent_name = if task.agent == crate::types::AgentKind::Custom {
        task.custom_agent_name.as_deref().unwrap_or("custom")
    } else {
        task.agent.as_str()
    };
    bail!(
        "Task {} uses '{}' in one-shot print mode and cannot consume interactive input; {} for {}",
        task.id,
        agent_name,
        command.refusal(),
        command.invocation()
    )
}

fn read_message(message: Option<&str>, file: Option<&str>) -> Result<String> {
    if let Some(path) = file {
        return std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read reply file: {path}"));
    }
    if let Some(message) = message {
        return Ok(message.to_string());
    }

    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("Failed to read from stdin")?;
    Ok(buf)
}

fn wait_for_ack<F>(
    store: &Store,
    task_id: &str,
    message_id: i64,
    timeout: Duration,
    poll_interval: Duration,
    on_poll: &mut F,
) -> Result<ReplyOutcome>
where
    F: FnMut(i64),
{
    let deadline = Instant::now() + timeout;
    let mut delivered = false;
    loop {
        let messages = store.list_messages_for_task(task_id)?;
        let message = messages
            .into_iter()
            .find(|entry| entry.id == message_id)
            .ok_or_else(|| anyhow!("Reply message {message_id} disappeared for task {task_id}"))?;
        delivered |= message.delivered_at.is_some();
        if message.acked_at.is_some() {
            return Ok(ReplyOutcome::Acked { delivered });
        }
        if Instant::now() >= deadline {
            return Ok(ReplyOutcome::TimedOut { delivered });
        }
        on_poll(message_id);
        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
#[path = "reply_tests.rs"]
mod tests;
