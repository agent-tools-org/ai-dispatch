// Component-aware usage pricing and task estimates from persisted completion metadata.
// Exports TokenUsage, estimate_usage_cost, task_cost; depends on cost resolution and Store.

use super::{estimate_cost, resolve_pricing};
use crate::store::Store;
use crate::types::{AgentKind, EventKind, Task};
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
            + usage.cache_creation as f64 * blended)
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
    if matches!(task.agent, AgentKind::Codex | AgentKind::Claude) {
        let events = store.get_events(task.id.as_str())?;
        if let Some(metadata) = events
            .iter()
            .rev()
            .find(|event| event.event_kind == EventKind::Completion)
            .and_then(|event| event.metadata.as_ref())
        {
            if let Some(cost) = metadata.get("cost_usd").and_then(Value::as_f64) {
                return Ok(Some(cost));
            }
            if let Some(usage) = usage_from_metadata(metadata, task.agent) {
                return Ok(estimate_usage_cost(usage, task.costing_model(), task.agent));
            }
        }
    }
    Ok(task
        .cost_usd
        .or_else(|| estimate_cost(task.tokens.unwrap_or(0), task.costing_model(), task.agent)))
}

#[cfg(test)]
#[path = "components_tests.rs"]
mod tests;
