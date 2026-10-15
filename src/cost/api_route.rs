// Reported API-account costs with live/final token estimates when no result survives.
// Persists only usage evidence, reusing component pricing and existing task estimates.
use super::components::{TokenUsage, estimate_usage_cost};
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, EventKind, Task, TaskEvent, TaskId};
use anyhow::Result;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(crate) fn task_cost(store: &Store, task: &Task) -> Result<Option<f64>> {
    let events = store.get_events(task.id.as_str())?;
    if let Some(cost) = events
        .iter()
        .rev()
        .filter(|event| event.event_kind == EventKind::Completion)
        .find_map(|event| event.metadata.as_ref()?.get("cost_usd")?.as_f64())
    {
        return Ok(Some(cost));
    }
    if task.cost_usd.is_some() {
        return Ok(task.cost_usd);
    }
    if let Some(cost) = usage_estimate(task, &events).0 {
        return Ok(Some(cost));
    }
    let mut claude = task.clone();
    claude.agent = AgentKind::Claude;
    super::task_cost(store, &claude)
}

fn usage_estimate(task: &Task, events: &[TaskEvent]) -> (Option<f64>, Option<i64>) {
    let mut messages = BTreeMap::new();
    for event in events {
        let Some(metadata) = &event.metadata else { continue };
        let raw = metadata
            .get("provider_stdout")
            .and_then(Value::as_str)
            .and_then(|line| serde_json::from_str::<Value>(line).ok());
        let usage = metadata.get("api_route_usage").or_else(|| {
            raw.as_ref()
                .filter(|value| value["type"] == "assistant")?
                .pointer("/message/usage")
        });
        let Some(usage) = usage else { continue };
        let key = metadata["usage_message_id"]
            .as_str()
            .or_else(|| raw.as_ref()?.pointer("/message/id")?.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| usage.to_string());
        let model = metadata["model"]
            .as_str()
            .or_else(|| raw.as_ref()?.pointer("/message/model")?.as_str())
            .map(str::to_owned);
        messages.insert(key, (usage.clone(), model));
    }
    let mut cost = None;
    let mut tokens = None;
    for (usage, model) in messages.values() {
        let count = |key: &str| usage[key].as_i64().unwrap_or(0).max(0);
        let usage = TokenUsage {
            uncached_input: count("input_tokens"),
            output: count("output_tokens"),
            cached_input: count("cache_read_input_tokens"),
            cache_creation: count("cache_creation_input_tokens"),
        };
        *tokens.get_or_insert(0) +=
            usage.uncached_input + usage.output + usage.cached_input + usage.cache_creation;
        let estimate = estimate_usage_cost(usage, model.as_deref(), AgentKind::Claude)
            .or_else(|| estimate_usage_cost(usage, task.costing_model(), AgentKind::Claude));
        if let Some(estimate) = estimate {
            *cost.get_or_insert(0.0) += estimate;
        }
    }
    (cost, tokens)
}

pub(crate) fn observe_usage(
    store: &Store,
    id: &TaskId,
    line: &str,
    info: &mut CompletionInfo,
) -> Result<bool> {
    let cleaned = crate::runtime::watcher::strip_terminal_escapes(line);
    let Ok(value) = serde_json::from_str::<Value>(cleaned.trim()) else {
        return Ok(false);
    };
    let metadata = if value["type"] == "assistant" && value["message"]["usage"].is_object() {
        json!({"api_route_usage": value["message"]["usage"],
            "usage_message_id": value["message"]["id"].as_str().map(str::to_owned)
                .unwrap_or_else(|| value["message"]["usage"].to_string()),
            "model": value["message"]["model"]})
    } else if value["type"] == "result" && value["total_cost_usd"].is_number() {
        json!({"cost_usd": value["total_cost_usd"]})
    } else {
        return Ok(false);
    };
    let kind = if metadata.get("cost_usd").is_some() {
        EventKind::Completion
    } else {
        EventKind::Reasoning
    };
    store.insert_agent_event(&TaskEvent {
        task_id: id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: kind,
        detail: "API route usage".into(),
        metadata: Some(metadata),
    })?;
    if let Some(task) = store.get_task(id.as_str())? {
        info.cost_usd = task_cost(store, &task)?;
        let (_, tokens) = usage_estimate(&task, &store.get_events(id.as_str())?);
        if tokens.is_some() {
            info.tokens = tokens;
        }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "api_route_tests.rs"]
mod tests;
