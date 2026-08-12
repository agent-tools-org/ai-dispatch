// TUI labels for operator waiting and agent activity states.
// Exports activity_label and reasoning_indicator_supported for renderers.
// Deps: Task/TaskEvent types and chrono timestamps.

use chrono::{DateTime, Local};

use crate::types::{AgentKind, EventKind, TaskEvent, TaskStatus};

pub(crate) fn reasoning_indicator_supported(agent: AgentKind) -> bool {
    // These adapters emit EventKind::Reasoning: the JSON/streaming adapters
    // plus the OpenCode-compatible Kilo/MiMoCode delegates and custom JSONL.
    matches!(
        agent,
        AgentKind::Gemini
            | AgentKind::Qwen
            | AgentKind::Codex
            | AgentKind::Copilot
            | AgentKind::OpenCode
            | AgentKind::CommandCode
            | AgentKind::Cursor
            | AgentKind::Kilo
            | AgentKind::MiMoCode
            | AgentKind::Droid
            | AgentKind::Oz
            | AgentKind::Claude
            | AgentKind::Custom
    )
}

pub(crate) fn activity_label(
    status: TaskStatus,
    agent: AgentKind,
    task_id: &str,
    latest: Option<&TaskEvent>,
) -> String {
    if status == TaskStatus::AwaitingInput {
        return waiting_label(task_id, latest);
    }
    if status == TaskStatus::Running
        && reasoning_indicator_supported(agent)
        && latest.is_some_and(|event| event.event_kind == EventKind::Reasoning)
    {
        let age = latest.map_or_else(|| "unknown".to_string(), elapsed_since_event);
        return format!("THINKING · reasoning · {age} ago");
    }
    last_output_label(latest)
}

fn waiting_label(task_id: &str, latest: Option<&TaskEvent>) -> String {
    let Some(event) = latest else {
        return format!("WAITING FOR OPERATOR · age unknown · aid respond {task_id} \"...\"");
    };
    let question = event
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("awaiting_prompt"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(event.detail.as_str());
    format!(
        "WAITING FOR OPERATOR · {} · {} · aid respond {task_id} \"...\"",
        elapsed_since_event(event),
        question
    )
}

fn last_output_label(latest: Option<&TaskEvent>) -> String {
    let Some(event) = latest else {
        return "Last output: none · no output yet".to_string();
    };
    format!(
        "Last output: {} · {} ago",
        event.event_kind.as_str(),
        elapsed_since_event(event)
    )
}

fn elapsed_since_event(event: &TaskEvent) -> String {
    format_elapsed(Local::now(), event.timestamp)
}

fn format_elapsed(now: DateTime<Local>, then: DateTime<Local>) -> String {
    let seconds = (now - then).num_seconds().max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, EventKind, TaskEvent, TaskId, TaskStatus};
    use chrono::{Duration, Local};

    fn event(kind: EventKind, detail: &str, age_seconds: i64) -> TaskEvent {
        TaskEvent {
            task_id: TaskId("t-state".to_string()),
            timestamp: Local::now() - Duration::seconds(age_seconds),
            event_kind: kind,
            detail: detail.to_string(),
            metadata: None,
        }
    }

    #[test]
    fn reasoning_indicator_capabilities_are_explicit_per_agent() {
        let expected = [
            (AgentKind::Gemini, true),
            (AgentKind::Qwen, true),
            (AgentKind::Codex, true),
            (AgentKind::Copilot, true),
            (AgentKind::OpenCode, true),
            (AgentKind::CommandCode, true),
            (AgentKind::Cursor, true),
            (AgentKind::Kilo, true),
            (AgentKind::MiMoCode, true),
            (AgentKind::Droid, true),
            (AgentKind::Oz, true),
            (AgentKind::Claude, true),
            (AgentKind::Antigravity, false),
            (AgentKind::Grok, false),
            (AgentKind::Custom, true),
        ];

        for (agent, supported) in expected {
            assert_eq!(reasoning_indicator_supported(agent), supported, "{agent:?}");
        }
    }

    #[test]
    fn supported_reasoning_event_renders_thinking() {
        let task_id = TaskId("t-thinking".to_string());
        let task = (TaskStatus::Running, AgentKind::Codex, task_id);
        let latest = event(EventKind::Reasoning, "checking the route", 12);

        assert_eq!(
            activity_label(task.0, task.1, task.2.as_str(), Some(&latest)),
            "THINKING · reasoning · 12s ago"
        );
    }

    #[test]
    fn unsupported_reasoning_event_degrades_to_last_output() {
        let latest = event(EventKind::Reasoning, "buffered output", 12);

        assert_eq!(
            activity_label(TaskStatus::Running, AgentKind::Grok, "t-buffered", Some(&latest)),
            "Last output: reasoning · 12s ago"
        );
    }

    #[test]
    fn awaiting_input_surfaces_question_age_and_response_command() {
        let mut latest = event(EventKind::Reasoning, "raw terminal prompt", 192);
        latest.metadata = Some(serde_json::json!({
            "awaiting_input": true,
            "awaiting_prompt": "Should I proceed?"
        }));

        assert_eq!(
            activity_label(
                TaskStatus::AwaitingInput,
                AgentKind::Codex,
                "t-wait",
                Some(&latest)
            ),
            "WAITING FOR OPERATOR · 3m12s · Should I proceed? · aid respond t-wait \"...\""
        );
    }
}
