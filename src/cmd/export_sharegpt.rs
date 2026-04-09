// ShareGPT JSONL exporter for fine-tuning data.
// Exports: export_sharegpt().
// Deps: paths, skills, store, templates, types, workgroup, serde_json.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskStatus};

pub fn export_sharegpt(store: &Store, task_id: &str, output: Option<&str>) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{task_id}' not found"))?;
    if !matches!(task.status, TaskStatus::Done | TaskStatus::Merged) {
        bail!("ShareGPT export only supports successful tasks");
    }
    let record = ShareGptRecord {
        conversations: vec![
            ShareGptMessage::new("system", resolved_prompt(store, &task)?),
            ShareGptMessage::new("human", task.prompt.clone()),
            ShareGptMessage::new("gpt", agent_response(&read_transcript(task_id)?, &store.get_events(task_id)?)),
        ],
    };
    let body = format!(
        "{}\n",
        serde_json::to_string(&record).context("Failed to serialize ShareGPT export")?
    );
    if let Some(path) = output {
        std::fs::write(path, body).with_context(|| format!("Failed to write ShareGPT export to {path}"))?;
    } else {
        print!("{body}");
    }
    Ok(())
}

fn resolved_prompt(store: &Store, task: &Task) -> Result<String> {
    if let Some(prompt) = task.resolved_prompt.as_ref() {
        return Ok(prompt.clone());
    }
    let workgroup = task
        .workgroup_id
        .as_deref()
        .map(|id| store.get_workgroup(id))
        .transpose()?
        .flatten();
    let milestones = task
        .workgroup_id
        .as_deref()
        .map(|id| store.get_workgroup_milestones(id))
        .transpose()?
        .unwrap_or_default();
    let skills = crate::skills::auto_skills(&task.agent, task.worktree_path.is_some())
        .into_iter()
        .map(|skill| {
            crate::skills::resolve_skill_content(skill.as_ref())
                .unwrap_or_else(|err| format!("[missing skill: {skill}: {err}]"))
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut prompt = crate::workgroup::compose_prompt(
        &task.prompt,
        None,
        workgroup.as_ref(),
        &milestones,
        &[],
    );
    if !skills.is_empty() {
        prompt = format!("{prompt}\n\n--- Methodology ---\n{skills}");
    }
    Ok(crate::templates::inject_milestone_prompt(&prompt))
}

fn read_transcript(task_id: &str) -> Result<String> {
    let path = paths::transcript_path(task_id);
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(content),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(err).with_context(|| format!("Failed to read transcript {}", path.display())),
    }
}

fn agent_response(transcript: &str, events: &[TaskEvent]) -> String {
    structured_transcript(transcript)
        .or_else(|| {
            let trimmed = transcript.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| fallback_response(events))
}

fn structured_transcript(transcript: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut parsed = false;
    for line in transcript.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        parsed = true;
        if let Some(text) = assistant_text(&value) {
            parts.push(text);
        }
        if let Some(text) = function_call_text(&value) {
            parts.push(format!("function_call: {text}"));
        }
        if let Some(text) = function_result_text(&value) {
            parts.push(format!("function_result: {text}"));
        }
    }
    parsed.then(|| parts.join("\n\n")).filter(|text| !text.trim().is_empty())
}

fn fallback_response(events: &[TaskEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match event.event_kind {
            EventKind::ToolCall => Some(format!("function_call: {}", event.detail)),
            EventKind::Reasoning | EventKind::Milestone | EventKind::Completion => Some(event.detail.clone()),
            EventKind::Error => Some(format!("[error] {}", event.detail)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn assistant_text(value: &Value) -> Option<String> {
    match value.get("type").and_then(Value::as_str) {
        Some("message")
            if value.get("role").and_then(Value::as_str) == Some("assistant")
                && value.get("delta").and_then(Value::as_bool) != Some(true) =>
        {
            extract_text(value.get("content")?)
        }
        Some("assistant") => extract_text(value.pointer("/message/content")?),
        Some("assistant.message") => value.pointer("/data/content").and_then(Value::as_str).map(str::to_string),
        Some("item.completed")
            if value.pointer("/item/type").and_then(Value::as_str) == Some("agent_message") =>
        {
            value.pointer("/item/text").and_then(Value::as_str).map(str::to_string)
        }
        Some("text") => value.pointer("/part/text").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

fn function_call_text(value: &Value) -> Option<String> {
    matches!(value.get("type").and_then(Value::as_str), Some("tool_use" | "tool_call" | "function_call"))
        .then(|| format_tool_payload(value))
        .flatten()
}

fn function_result_text(value: &Value) -> Option<String> {
    (value.get("type").and_then(Value::as_str) == Some("tool_result"))
        .then(|| format_tool_result(value))
        .flatten()
}

fn format_tool_payload(value: &Value) -> Option<String> {
    let tool = tool_name(value).unwrap_or("tool");
    let payload = [
        value.pointer("/part/state/output"),
        value.get("output"),
        value.get("arguments"),
        value.pointer("/functionCall/args"),
        value.get("parameters"),
        value.get("input"),
        value.pointer("/tool_call"),
    ]
    .into_iter()
    .flatten()
    .find_map(stringify);
    payload.map(|payload| format!("{tool} {payload}"))
}

fn format_tool_result(value: &Value) -> Option<String> {
    let tool = tool_name(value).unwrap_or("tool");
    let payload = [
        value.pointer("/part/state/output"),
        value.get("output"),
        value.pointer("/result/output"),
        value.pointer("/result/content"),
        value.get("content"),
        value.get("text"),
    ]
    .into_iter()
    .flatten()
    .find_map(stringify)?;
    Some(format!("{tool} {payload}"))
}

fn tool_name(value: &Value) -> Option<&str> {
    value.get("tool_name")
        .and_then(Value::as_str)
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| value.pointer("/part/tool").and_then(Value::as_str))
        .or_else(|| value.pointer("/functionCall/name").and_then(Value::as_str))
        .or_else(|| value.get("tool").and_then(Value::as_str))
}

fn extract_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let parts = items.iter().filter_map(extract_text).collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.concat())
        }
        Value::Object(map) => ["text", "content", "parts"]
            .into_iter()
            .find_map(|key| map.get(key).and_then(extract_text)),
        _ => None,
    }
}

fn stringify(value: &Value) -> Option<String> {
    extract_text(value).or_else(|| (!value.is_null()).then(|| value.to_string()))
}

#[derive(Serialize, Deserialize)]
struct ShareGptRecord {
    conversations: Vec<ShareGptMessage>,
}

#[derive(Serialize, Deserialize)]
struct ShareGptMessage {
    from: String,
    value: String,
}

impl ShareGptMessage {
    fn new(from: &str, value: String) -> Self {
        Self {
            from: from.to_string(),
            value,
        }
    }
}

#[cfg(test)]
#[path = "export_sharegpt_tests.rs"]
mod tests;
