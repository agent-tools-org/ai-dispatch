// Handler for `aid explain <task-id>` — summarize a prior task via AI.
// Gathers task metadata, event history, stderr, and log tails before dispatch.

use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::paths;
use crate::store::Store;
use crate::types::{Task, TaskEvent};

pub struct ExplainArgs {
    pub task_id: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub output: Option<String>,
}

pub async fn run(store: Arc<Store>, args: ExplainArgs) -> Result<()> {
    let ExplainArgs {
        task_id,
        agent,
        model,
        output,
    } = args;
    let task = store
        .get_task(&task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task '{}' not found", task_id))?;
    let events = store.get_events(&task_id)?;
    let stderr_tail = read_tail(&paths::stderr_path(&task_id), 30, "stderr unavailable");
    let log_tail = read_tail(&paths::log_path(&task_id), 50, "log unavailable");
    let context = build_context(&task, &events, &stderr_tail, &log_tail);
    let explain_prompt = build_explain_prompt(&context);
    let agent_name = agent.unwrap_or_else(|| "gemini".to_string());

    println!("[explain] Analyzing task {} via {}...", task_id, agent_name);
    let _ = run::run(
        store,
        RunArgs {
            agent_name,
            prompt: explain_prompt,
            dir: None,
            output,
            model,
            worktree: None,
            group: None,
            verify: None,
            retry: 0,
            context: vec![],
            background: false,
            parent_task_id: Some(task_id),
        },
    )
    .await?;
    Ok(())
}

fn build_context(task: &Task, events: &[TaskEvent], stderr_tail: &str, log_tail: &str) -> String {
    format!(
        "[Task Info]\n{}\n\n[Events Timeline]\n{}\n\n[Stderr Tail]\n{}\n\n[Log Tail]\n{}",
        format_task_info(task),
        format_events(events),
        stderr_tail,
        log_tail,
    )
}

fn build_explain_prompt(context: &str) -> String {
    format!(
        concat!(
            "You are explaining a prior `aid` task execution.\n",
            "Analyze the context and answer with these sections:\n",
            "1. Summary: exactly one sentence.\n",
            "2. Intent: what the task tried to do.\n",
            "3. What Happened: concise timeline of execution.\n",
            "4. Root Cause: likely reason for failure, or say that no failure is evident.\n",
            "Be concrete, use the evidence in the artifacts, and say when evidence is missing.\n\n",
            "[Execution Context]\n",
            "{}"
        ),
        context,
    )
}

fn format_task_info(task: &Task) -> String {
    let mut lines = vec![
        format!("Task ID: {}", task.id),
        format!("Agent: {}", task.agent),
        format!("Status: {}", task.status.label()),
        format!("Prompt: {}", task.prompt),
        format!(
            "Parent Task ID: {}",
            task.parent_task_id.as_deref().unwrap_or("(none)")
        ),
        format!("Model: {}", task.model.as_deref().unwrap_or("(none)")),
        format!("Created At: {}", task.created_at.to_rfc3339()),
    ];

    lines.push(format!(
        "Completed At: {}",
        task.completed_at
            .map(|value| value.to_rfc3339())
            .unwrap_or_else(|| "(not completed)".to_string())
    ));
    lines.push(format!(
        "Duration: {}",
        task.duration_ms
            .map(|value| format!("{value} ms"))
            .unwrap_or_else(|| "(unknown)".to_string())
    ));
    lines.push(format!(
        "Tokens: {}",
        task.tokens
            .map(|value| value.to_string())
            .unwrap_or_else(|| "(unknown)".to_string())
    ));
    lines.push(format!(
        "Cost USD: {}",
        task.cost_usd
            .map(|value| format!("{value:.4}"))
            .unwrap_or_else(|| "(unknown)".to_string())
    ));
    lines.push(format!(
        "Worktree: {}",
        task.worktree_path.as_deref().unwrap_or("(none)")
    ));
    lines.push(format!(
        "Output Path: {}",
        task.output_path.as_deref().unwrap_or("(none)")
    ));
    lines.push(format!(
        "Stderr Path: {}",
        paths::stderr_path(task.id.as_str()).display()
    ));
    lines.push(format!(
        "Log Path: {}",
        paths::log_path(task.id.as_str()).display()
    ));
    lines.join("\n")
}

fn format_events(events: &[TaskEvent]) -> String {
    if events.is_empty() {
        return "(no events recorded)".to_string();
    }

    events
        .iter()
        .map(format_event)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_event(event: &TaskEvent) -> String {
    let metadata = event
        .metadata
        .as_ref()
        .map(|value| format!(" metadata={value}"))
        .unwrap_or_default();
    format!(
        "{} {} {}{}",
        event.timestamp.to_rfc3339(),
        event.event_kind.as_str(),
        event.detail,
        metadata,
    )
}

fn read_tail(path: &Path, limit: usize, unavailable: &str) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return unavailable.to_string();
    };
    let content = String::from_utf8_lossy(&bytes);
    let tail = tail_lines(&content, limit);
    if tail.is_empty() {
        unavailable.to_string()
    } else {
        tail
    }
}

fn tail_lines(content: &str, limit: usize) -> String {
    content
        .lines()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{build_explain_prompt, tail_lines};

    #[test]
    fn tail_lines_keeps_only_requested_suffix() {
        let content = "a\nb\nc\nd";
        assert_eq!(tail_lines(content, 2), "c\nd");
    }

    #[test]
    fn explain_prompt_embeds_execution_context() {
        let prompt = build_explain_prompt("task context");
        assert!(prompt.contains("Summary: exactly one sentence."));
        assert!(prompt.contains("[Execution Context]\ntask context"));
    }
}
