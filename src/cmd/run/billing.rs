// Dispatch-time billing warnings and persistent overage evidence.
// Uses the selected route after substitution; dry runs never reach this hook.

use crate::routing::quota::{billing::BillingState, live};
use crate::store::Store;
use crate::types::{AgentKind, EventKind, TaskEvent, TaskId};

pub(super) fn record(store: &Store, task_id: &TaskId, agent: AgentKind) -> anyhow::Result<()> {
    let name = store
        .get_task(task_id.as_str())?
        .and_then(|task| task.custom_agent_name);
    if agent == AgentKind::Codex
        && name
            .as_deref()
            .is_some_and(|n| n.starts_with("codex-provider:"))
    {
        return Ok(());
    }
    let api = name
        .map(|name| crate::config::api_route::find(&name))
        .transpose()?
        .flatten();
    let billing = match api {
        Some(route) => Some(crate::routing::api_budget::for_route(&route, Some(store))?.billing),
        None => live::billing(&agent),
    };
    let Some(billing) = billing else {
        return Ok(());
    };
    if matches!(billing.state, BillingState::Overage | BillingState::Exhausted) {
        aid_warn!("[aid] {} billing: {}", agent.as_str(), billing.label());
    }
    if billing.state != BillingState::Overage {
        return Ok(());
    }
    if store.get_events(task_id.as_str())?.iter().any(|event| {
        event
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("billing_state"))
            .and_then(serde_json::Value::as_str)
            == Some("overage")
    }) {
        return Ok(());
    }
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Setup,
        detail: format!("billing: {}", billing.label()),
        metadata: Some(serde_json::json!({"billing_state": "overage", "billing": billing})),
    })?;
    Ok(())
}

#[cfg(test)]
#[path = "billing_tests.rs"]
mod tests;
