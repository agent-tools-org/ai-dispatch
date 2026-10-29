// Preserve one reviewable diagnostic for failures outside the fixed rules.
// Reads provider channels and records metadata without changing execution or routing.

use crate::routing::{auth_marker, capacity, quota::channel, rate_limit};
use crate::store::Store;
use crate::types::{AgentKind, EventKind, TaskEvent, TaskId};
use channel::Channel;

pub(super) fn record_unclassified(store: &Store, task_id: &TaskId) {
    let Ok(Some(task)) = store.get_task(task_id.as_str()) else {
        return;
    };
    let Ok(events) = store.get_events(task_id.as_str()) else {
        return;
    };
    if events.iter().any(|event| {
        event
            .metadata
            .as_ref()
            .is_some_and(|data| data["unclassified_failure"] == true)
    }) {
        return;
    }
    let stderr = crate::runtime::watcher::read_capped_stderr(task_id.as_str()).unwrap_or_default();
    let stream = channel::read_run_stream(task_id.as_str()).unwrap_or_default();
    let captured = events
        .iter()
        .filter_map(|event| event.metadata.as_ref()?.get("provider_stdout")?.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let stdout = format!("{stream}\n{captured}");
    if recognised(&stderr, task.agent, Channel::CliStderr)
        || recognised(&stdout, task.agent, Channel::CliStream)
    {
        return;
    }
    let excerpt = diagnostic_excerpt(&stderr, &stdout, task.agent);
    if excerpt.is_empty() {
        return;
    }
    let _ = store.insert_event(&TaskEvent::new(
        task_id.clone(),
        EventKind::Milestone,
        "Unclassified provider failure".to_string(),
    ).with_metadata(Some(serde_json::json!({
            "unclassified_failure": true,
            "agent": task.agent.as_str(),
            "provider_excerpt": excerpt,
        }))));
}

fn recognised(raw: &str, agent: AgentKind, source: Channel) -> bool {
    rate_limit::refusal_on_channel(raw, agent, source).is_some()
        || auth_marker::auth_failure_line(raw, agent, source).is_some()
        || capacity::classify(raw, agent, source).is_some()
        || (agent == AgentKind::Codex
            && source == Channel::CliStream
            && raw
                .lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .any(|value: serde_json::Value| {
                    crate::agent::codex::app_server::error_quota_message(&value).is_some()
                        || (value["type"] == "codex.rate_limits"
                            && crate::agent::codex::app_server::quota_message(
                                &value["rate_limits"],
                            )
                            .is_some())
                }))
}

fn diagnostic_excerpt(stderr: &str, stdout: &str, agent: AgentKind) -> String {
    let mut diagnostic =
        channel::provider_attributable(stderr, agent, Channel::CliStderr).cli_diagnostic;
    diagnostic.push_str(
        &channel::provider_attributable(stdout, agent, Channel::CliStream).cli_diagnostic,
    );
    // Typed non-quota errors retain their message even when their code rejects a text match.
    if agent == AgentKind::Codex {
        for value in stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        {
            if matches!(value["type"].as_str(), Some("error" | "turn.failed"))
                && let Some(message) = value["error"]["message"].as_str()
            {
                diagnostic.push_str(message);
                diagnostic.push('\n');
            }
        }
    }
    diagnostic.trim().chars().take(1000).collect()
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod tests;
