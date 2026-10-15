// Persist app-server availability and steer receipts alongside task state.
// Pending inbound messages remain the only source of steer requests.

use crate::{store::Store, support::paths, types::*};
use anyhow::{Result, bail};
use serde_json::{Value, json};

pub(super) fn marker(task_id: &str) -> std::path::PathBuf {
    paths::task_dir(task_id).join("codex-app-server.json")
}

pub(crate) fn active(task_id: &str) -> bool {
    let Some(value) = std::fs::read(marker(task_id))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return false;
    };
    let Some(pid) = value["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
    else {
        return false;
    };
    crate::runtime::process::monitor::activity::root_matches(pid, value["started"].as_str())
}

pub(super) struct Active(pub String);

impl Active {
    pub fn create(task_id: &str, pid: u32) -> Result<Self> {
        let started = crate::runtime::process::monitor::activity::root_started(pid);
        std::fs::write(
            marker(task_id),
            json!({"pid":pid,"started":started}).to_string(),
        )?;
        Ok(Self(task_id.to_string()))
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(marker(&self.0));
    }
}

pub(super) fn record(store: &Store, task: &TaskId, id: i64, result: Result<String>) -> Result<()> {
    let (accepted, text, turn) = match result {
        Ok(turn) => (true, format!("accepted by codex (turn {turn})"), Some(turn)),
        Err(error) => (false, error.to_string(), None),
    };
    store.insert_event(&TaskEvent {
        task_id: task.clone(),
        timestamp: chrono::Local::now(),
        event_kind: if accepted {
            EventKind::Milestone
        } else {
            EventKind::Error
        },
        detail: text.clone(),
        metadata: Some(json!({"codex_steer_message_id":id,
            "accepted":accepted,"turn_id":turn,"error":if accepted {None} else {Some(text)}})),
    })?;
    if accepted {
        store.mark_delivered(id)?;
    }
    Ok(())
}

pub(crate) fn receipt(store: &Store, task_id: &str, id: i64) -> Result<Option<String>> {
    for event in store.get_events(task_id)?.into_iter().rev() {
        let Some(metadata) = event.metadata else {
            continue;
        };
        if metadata["codex_steer_message_id"].as_i64() != Some(id) {
            continue;
        }
        if metadata["accepted"] == true {
            return Ok(metadata["turn_id"].as_str().map(str::to_string));
        }
        bail!(
            "{}",
            metadata["error"].as_str().unwrap_or("Codex steer failed")
        );
    }
    Ok(None)
}

pub(crate) fn wait_for_interrupt(task_id: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while active(task_id) && std::time::Instant::now() < deadline {
        std::thread::park_timeout(std::time::Duration::from_millis(25));
    }
}

pub(super) fn steer(thread: &str, turn: Option<&str>, text: &str) -> Result<Value> {
    let turn = turn.ok_or_else(|| anyhow::anyhow!("Codex has no active steerable turn"))?;
    Ok(json!({"threadId":thread,"expectedTurnId":turn,
        "input":[{"type":"text","text":text}]}))
}

pub(super) fn accepted(response: &Value, expected: &str) -> Result<String> {
    let turn = response["turnId"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing steer turnId"))?;
    anyhow::ensure!(
        turn == expected,
        "Codex steer turn id mismatch: expected {expected}, got {turn}"
    );
    Ok(turn.to_string())
}
