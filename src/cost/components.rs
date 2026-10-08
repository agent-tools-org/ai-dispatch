// Component-aware usage pricing and task estimates from persisted completion metadata.
// Exports TokenUsage, estimate_usage_cost, task_cost; depends on cost resolution and Store.

use super::{estimate_cost, resolve_pricing};
use crate::store::Store;
use crate::types::{AgentKind, CompletionInfo, EventKind, Task, TaskEvent, TaskId};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TokenUsage {
    pub(crate) uncached_input: i64,
    pub(crate) cached_input: i64,
    pub(crate) output: i64,
    pub(crate) cache_creation: i64,
}

pub(crate) fn estimate_usage_cost(
    usage: TokenUsage,
    model: Option<&str>,
    agent: AgentKind,
) -> Option<f64> {
    let pricing = resolve_pricing(model, agent)?;
    let blended = pricing.input_per_m * 0.7 + pricing.output_per_m * 0.3;
    Some(
        (usage.uncached_input as f64 * pricing.input_per_m
            + usage.cached_input as f64 * pricing.cached_input_per_m.unwrap_or(blended)
            + usage.output as f64 * pricing.output_per_m
            + usage.cache_creation as f64
                * pricing
                    .cache_creation_per_m
                    .unwrap_or(pricing.input_per_m * 1.25))
            / 1_000_000.0,
    )
}

fn token_count(metadata: &Value, name: &str) -> Option<i64> {
    metadata.get(name)?.as_i64().filter(|count| *count >= 0)
}

fn usage_from_metadata(metadata: &Value, agent: AgentKind) -> Option<TokenUsage> {
    let input = token_count(metadata, "input_tokens")?;
    let output = token_count(metadata, "output_tokens")?;
    let (cached_input, cache_creation) = match agent {
        AgentKind::Codex => (token_count(metadata, "cached_input_tokens")?, 0),
        AgentKind::Claude => (
            token_count(metadata, "cache_read_input_tokens")?,
            token_count(metadata, "cache_creation_input_tokens")?,
        ),
        _ => return None,
    };
    let uncached_input = if agent == AgentKind::Codex {
        input
            .checked_sub(cached_input)
            .filter(|count| *count >= 0)?
    } else {
        input
    };
    Some(TokenUsage {
        uncached_input,
        cached_input,
        output,
        cache_creation,
    })
}

pub(crate) fn task_cost(store: &Store, task: &Task) -> anyhow::Result<Option<f64>> {
    if let Some(cost) = recorded_component_cost(
        store,
        &task.id,
        task.tokens,
        task.costing_model(),
        task.agent,
    )? {
        return Ok(cost);
    }
    Ok(task
        .cost_usd
        .or_else(|| estimate_cost(task.tokens.unwrap_or(0), task.costing_model(), task.agent)))
}

fn recorded_component_cost(
    store: &Store,
    task_id: &TaskId,
    tokens: Option<i64>,
    model: Option<&str>,
    agent: AgentKind,
) -> anyhow::Result<Option<Option<f64>>> {
    if !matches!(agent, AgentKind::Codex | AgentKind::Claude) {
        return Ok(None);
    }
    let events = store.get_events(task_id.as_str())?;
    let metadata = events
        .iter()
        .rev()
        .filter(|event| event.event_kind == EventKind::Completion)
        .find_map(|event| {
            event.metadata.as_ref().filter(|metadata| {
                metadata.get("tokens").is_some() || metadata.get("cost_usd").is_some()
            })
        });
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    if tokens.is_none() || token_count(metadata, "tokens") != tokens {
        return Ok(None);
    }
    if let Some(cost) = metadata.get("cost_usd").and_then(Value::as_f64) {
        return Ok(Some(Some(cost)));
    }
    Ok(usage_from_metadata(metadata, agent).map(|usage| estimate_usage_cost(usage, model, agent)))
}

pub(crate) fn completion_cost(
    store: &Store,
    task_id: &TaskId,
    info: &CompletionInfo,
    model: Option<&str>,
    agent: AgentKind,
) -> anyhow::Result<Option<f64>> {
    if let Some(cost) = recorded_component_cost(store, task_id, info.tokens, model, agent)? {
        return Ok(cost);
    }
    if let Some(cost) = info.cost_usd {
        return Ok(Some(cost));
    }
    Ok(info
        .tokens
        .and_then(|tokens| estimate_cost(tokens, model, agent)))
}

pub(crate) fn apply_completion_usage_cost(
    store: &Store,
    task_id: &TaskId,
    info: &mut CompletionInfo,
    event: &TaskEvent,
    agent: AgentKind,
) -> anyhow::Result<()> {
    if event.event_kind != EventKind::Completion {
        return Ok(());
    }
    let Some(metadata) = event.metadata.as_ref() else {
        return Ok(());
    };
    if metadata.get("cost_usd").and_then(Value::as_f64).is_some() {
        return Ok(());
    }
    let Some(usage) = usage_from_metadata(metadata, agent) else {
        return Ok(());
    };
    let task = store.get_task(task_id.as_str())?;
    let model = info.model.as_deref().or_else(|| {
        task.as_ref()
            .and_then(|task| task.requested_model.as_deref())
    });
    info.cost_usd = estimate_usage_cost(usage, model, agent);
    Ok(())
}

#[cfg(test)]
#[path = "components_tests.rs"]
mod tests;
