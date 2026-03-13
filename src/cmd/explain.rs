// AI-powered task explanation: dispatches a research agent to analyze task artifacts.
// Called from show.rs when `aid show --explain` is used; exports: run_explain(), build_explain_context(), build_explain_prompt(); deps: cmd::run and cmd::show helpers.

use anyhow::Result;
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::cmd::show::{load_task, read_tail};
use crate::paths;
use crate::store::Store;
use crate::types::{Task, TaskEvent};
pub(crate) async fn run_explain(store: Arc<Store>, task_id: &str, agent: Option<String>, model: Option<String>) -> Result<()> {
    let task = load_task(&store, task_id)?;
    let events = store.get_events(task_id)?;
    let stderr = read_tail(&paths::stderr_path(task_id), 30, "stderr unavailable");
    let log = read_tail(&paths::log_path(task_id), 50, "log unavailable");
    let context = build_explain_context(&task, &events, &stderr, &log);
    let prompt = build_explain_prompt(&context);
    let agent_name = agent.unwrap_or_else(|| "gemini".to_string());

    println!("[explain] Analyzing task {task_id} via {agent_name}...");
    let _ = run::run(
        store,
        RunArgs {
            agent_name,
            prompt,
            repo: None,
            dir: None,
            output: None,
            model,
            worktree: None,
            base_branch: None,
            group: None,
            verify: None,
            max_duration_mins: None,
            retry: 0,
            context: vec![],
            skills: vec![],
            template: None,
            background: false,
            announce: true,
            parent_task_id: Some(task_id.to_string()),
            on_done: None,
            fallback: None,
            read_only: false,
            budget: false,
            session_id: None,
        },
    )
    .await?;
    Ok(())
}
pub(crate) fn build_explain_context(task: &Task, events: &[TaskEvent], stderr_tail: &str, log_tail: &str) -> String {
    format!(
        "[Task Info]\n{}\n\n[Events Timeline]\n{}\n\n[Stderr Tail]\n{}\n\n[Log Tail]\n{}",
        format_task_info(task),
        format_events(events),
        stderr_tail,
        log_tail,
    )
}
pub(crate) fn build_explain_prompt(context: &str) -> String {
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
    let completed = task
        .completed_at
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| "(not completed)".to_string());
    let duration = task
        .duration_ms
        .map(|value| format!("{value} ms"))
        .unwrap_or_else(|| "(unknown)".to_string());
    let tokens = task
        .tokens
        .map(|value| value.to_string())
        .unwrap_or_else(|| "(unknown)".to_string());
    let cost = task
        .cost_usd
        .map(|value| format!("{value:.4}"))
        .unwrap_or_else(|| "(unknown)".to_string());

    [
        format!("Task ID: {}", task.id),
        format!("Agent: {}", task.agent),
        format!("Status: {}", task.status.label()),
        format!("Prompt: {}", task.prompt),
        format!("Parent Task ID: {}", task.parent_task_id.as_deref().unwrap_or("(none)")),
        format!("Model: {}", task.model.as_deref().unwrap_or("(none)")),
        format!("Created At: {}", task.created_at.to_rfc3339()),
        format!("Completed At: {completed}"),
        format!("Duration: {duration}"),
        format!("Tokens: {tokens}"),
        format!("Cost USD: {cost}"),
        format!("Worktree: {}", task.worktree_path.as_deref().unwrap_or("(none)")),
        format!("Output Path: {}", task.output_path.as_deref().unwrap_or("(none)")),
        format!("Stderr Path: {}", paths::stderr_path(task.id.as_str()).display()),
        format!("Log Path: {}", paths::log_path(task.id.as_str()).display()),
    ]
    .join("\n")
}

fn format_events(events: &[TaskEvent]) -> String {
    if events.is_empty() {
        return "(no events recorded)".to_string();
    }
    events
        .iter()
        .map(|event| {
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
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explain_prompt_embeds_execution_context() {
        let prompt = build_explain_prompt("task context");
        assert!(prompt.contains("Summary: exactly one sentence."));
        assert!(prompt.contains("[Execution Context]\ntask context"));
    }
}
