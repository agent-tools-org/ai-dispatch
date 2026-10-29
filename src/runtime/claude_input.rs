// Claude streaming stdin transport with durable, UUID-matched replay receipts.
// Retains piped watcher billing checks; live markers exclude legacy workers.
use crate::{store::Store, types::*};
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
#[path = "claude_input_write.rs"]
mod write;

fn marker(task: &str) -> std::path::PathBuf {
    crate::support::paths::task_dir(task).join("claude-input.json")
}

pub(crate) fn active(task: &str) -> bool {
    let value = std::fs::read(marker(task))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    value.is_some_and(|value| {
        value["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .is_some_and(|pid| {
                crate::runtime::process::monitor::activity::root_matches(
                    pid,
                    value["started"].as_str(),
                )
            })
    })
}

pub(crate) fn supports(agent: &dyn crate::agent::Agent) -> bool {
    agent.kind() == AgentKind::Claude || agent.api_route().is_some()
}

pub(crate) fn prepare(
    agent: &dyn crate::agent::Agent,
    command: &mut tokio::process::Command,
) -> Result<Option<String>> {
    if !supports(agent) {
        return Ok(None);
    }
    let original = command.as_std();
    let args: Vec<_> = original.get_args().collect();
    let index = args
        .iter()
        .position(|arg| *arg == "-p")
        .ok_or_else(|| anyhow!("Missing Claude print flag"))?;
    let prompt = args
        .get(index + 1)
        .and_then(|arg| arg.to_str())
        .ok_or_else(|| anyhow!("Missing Claude initial prompt"))?
        .to_string();
    let mut next = std::process::Command::new(original.get_program());
    for (offset, arg) in args.iter().enumerate() {
        if offset != index + 1 {
            next.arg(arg);
        }
        if offset == 0 && original.get_program() == "container" {
            next.arg("-i");
        }
    }
    next.args(["--input-format", "stream-json", "--replay-user-messages"]);
    if let Some(dir) = original.get_current_dir() {
        next.current_dir(dir);
    }
    for (key, value) in original.get_envs() {
        match value {
            Some(value) => {
                next.env(key, value);
            }
            None => {
                next.env_remove(key);
            }
        }
    }
    *command = next.into();
    Ok(Some(prompt))
}

fn envelope(uuid: &str, text: &str) -> String {
    format!(
        "{}\n",
        json!({"type":"user", "uuid":uuid, "session_id":"",
        "parent_tool_use_id":null, "message":{"role":"user", "content":text}})
    )
}

fn record(
    store: &Store,
    task: &TaskId,
    id: i64,
    uuid: &str,
    accepted: bool,
    sent: bool,
) -> Result<()> {
    store.insert_event(&TaskEvent::new(
        task.clone(),
        EventKind::Milestone,
        if accepted {
            "accepted by claude"
        } else if sent {
            "Claude input sent, unconfirmed"
        } else {
            "Claude input delivery attempted, unconfirmed"
        }
        .into(),
    ).with_metadata(Some(json!({"claude_steer_message_id":id, "message_uuid":uuid,
            "accepted":if accepted {Some(true)} else {None}, "unconfirmed":!accepted}))))?;
    if accepted {
        store.mark_delivered(id)?;
    }
    Ok(())
}

pub(crate) fn receipt(store: &Store, task: &str, id: i64) -> Result<bool> {
    Ok(store.get_events(task)?.iter().any(|event| {
        event.metadata.as_ref().is_some_and(|value| {
            value["claude_steer_message_id"] == id && value["accepted"] == true
        })
    }))
}

struct Pending {
    id: Option<i64>,
    acknowledged: bool,
    sent: Instant,
}

pub(crate) struct Session {
    stdin: Option<tokio::process::ChildStdin>,
    task: String,
    pending: BTreeMap<String, Pending>,
    result: bool,
}

impl Session {
    pub(crate) async fn start(
        child: &mut tokio::process::Child,
        task: &TaskId,
        store: &Store,
        prompt: Option<&str>,
    ) -> Result<Option<Self>> {
        let Some(prompt) = prompt else {
            return Ok(None);
        };
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("Missing Claude stdin"))?;
        let mut session = Self {
            stdin: Some(stdin),
            task: task.to_string(),
            pending: BTreeMap::new(),
            result: false,
        };
        session.send(None, prompt, store, task).await?;
        if session.stdin.is_none() {
            return Ok(None);
        }
        let pid = std::process::id();
        std::fs::create_dir_all(crate::support::paths::task_dir(task.as_str()))?;
        std::fs::write(
            marker(task.as_str()),
            json!({"pid":pid,
            "started":crate::runtime::process::monitor::activity::root_started(pid)})
            .to_string(),
        )?;
        Ok(Some(session))
    }

    async fn send(
        &mut self,
        id: Option<i64>,
        text: &str,
        store: &Store,
        task: &TaskId,
    ) -> Result<()> {
        let uuid = match id {
            Some(id) => format!("00000000-0000-4000-8000-{:012x}", id),
            None => "00000000-0000-4000-8000-000000000000".to_string(),
        };
        if let Some(id) = id {
            record(store, task, id, &uuid, false, false)?;
        }
        self.pending.insert(
            uuid.clone(),
            Pending {
                id,
                acknowledged: false,
                sent: Instant::now(),
            },
        );
        self.result = false;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("Claude stdin closed"))?;
        let bytes = envelope(&uuid, text);
        let written = if id.is_none() {
            write::initial(stdin, bytes.as_bytes())
                .await
                .context("Failed to write Claude initial prompt")?
        } else {
            write::send(stdin, bytes.as_bytes(), Duration::from_secs(5)).await
        };
        if !written {
            self.stdin.take();
            let _ = std::fs::remove_file(marker(&self.task));
            return Ok(());
        }
        if let Some(id) = id {
            record(store, task, id, &uuid, false, true)?;
        }
        Ok(())
    }

    pub(crate) fn observe(&mut self, store: &Store, task: &TaskId, line: &str) -> Result<()> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Ok(());
        };
        if value["type"] == "user"
            && value["isReplay"] == true
            && value["parent_tool_use_id"].is_null()
            && value["message"]["role"] == "user"
            && let Some(uuid) = value["uuid"].as_str()
            && let Some(pending) = self.pending.get_mut(uuid)
            && !pending.acknowledged
        {
            if let Some(id) = pending.id {
                record(store, task, id, uuid, true, true)?;
            }
            pending.acknowledged = true;
            self.result = false;
        }
        if value["type"] == "result" {
            self.result = true;
        }
        if value["type"] == "assistant" {
            self.result = false;
        }
        Ok(())
    }

    pub(crate) async fn poll(&mut self, store: &Store, task: &TaskId) -> Result<()> {
        if self.stdin.is_none() {
            return Ok(());
        }
        let events = store.get_events(task.as_str())?;
        for message in store.list_messages_for_task(task.as_str())? {
            if message.direction != MessageDirection::In || message.delivered_at.is_some() {
                continue;
            }
            if events.iter().any(|event| {
                event
                    .metadata
                    .as_ref()
                    .is_some_and(|value| value["claude_steer_message_id"] == message.id)
            }) {
                continue;
            }
            self.send(Some(message.id), &message.content, store, task)
                .await?;
            if self.stdin.is_none() {
                break;
            }
        }
        if self.result
            && self.pending.values().all(|pending| {
                pending.acknowledged || pending.sent.elapsed() >= Duration::from_secs(30)
            })
        {
            std::fs::remove_file(marker(&self.task))?;
            self.stdin.take();
        }
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(marker(&self.task));
    }
}

#[cfg(test)]
#[path = "claude_input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "claude_input_exit_tests.rs"]
mod exit_tests;

#[cfg(test)]
#[path = "claude_input_route_tests.rs"]
mod route_tests;
