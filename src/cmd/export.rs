// Task export handlers for markdown/json and ShareGPT JSONL output.
// Exports: ExportArgs, ExportFormat, run().
// Deps: show::worktree_diff, export_sharegpt, Store, Task/TaskEvent.
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::{fs, sync::Arc};
use crate::{cmd::show::worktree_diff, paths, store::Store, types::{Task, TaskEvent}};

#[path = "export_sharegpt.rs"]
mod export_sharegpt;

pub enum ExportFormat {
    Markdown,
    Json,
}
impl ExportFormat {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "md" | "markdown" => Ok(Self::Markdown),
            "json" => Ok(Self::Json),
            other => Err(anyhow!("Unsupported export format '{other}'")),
        }
    }
}
pub struct ExportArgs {
    pub task_id: String,
    pub format: ExportFormat,
    pub sharegpt: bool,
    pub output: Option<String>,
}
pub async fn run(store: Arc<Store>, args: ExportArgs) -> Result<()> {
    if args.sharegpt {
        return export_sharegpt::export_sharegpt(store.as_ref(), &args.task_id, args.output.as_deref());
    }
    let task = load_task(&store, &args.task_id)?;
    let events = store.get_events(&args.task_id)?;
    let output = read_output(&task)?;
    let diff = worktree_diff(&task, &args.task_id)?
        .trim_start_matches('\n')
        .to_string();
    let timeline = format_event_timeline(&events);
    let body = match args.format {
        ExportFormat::Markdown => build_markdown(&task, &timeline, &output, &diff),
        ExportFormat::Json => build_json(&task, &timeline, &output, &diff)?,
    };
    if let Some(file) = args.output {
        fs::write(file, body)?;
    } else {
        print!("{body}");
    }
    Ok(())
}
fn build_markdown(task: &Task, timeline: &[String], output: &str, diff: &str) -> String {
    let header = format!(
        "# Task {}\n- Route: {}\n- Status: {}\n- Duration: {}\n- Cost: {}\n- Created: {}\n\n## Prompt\n",
        task.id,
        task.display_route(),
        task.status.as_str(),
        format_duration(task.duration_ms),
        format_cost(task.cost_usd),
        task.created_at.to_rfc3339(),
    );
    let mut out = header;
    out.push_str(&task.prompt);
    out.push_str("\n\n## Events\n");
    if timeline.is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(&timeline.join("\n"));
        out.push('\n');
    }
    out.push_str("\n## Output\n");
    if output.is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(output);
        out.push('\n');
    }
    out.push_str("\n## Diff\n```diff\n");
    out.push_str(diff);
    out.push_str("\n```\n");
    out
}
fn build_json(task: &Task, timeline: &[String], output: &str, diff: &str) -> Result<String> {
    let payload = ExportPayload {
        id: task.id.as_str(),
        agent: task.agent_display_name(),
        status: task.status.as_str(),
        duration: format_duration(task.duration_ms),
        cost: format_cost(task.cost_usd),
        created_at: task.created_at.to_rfc3339(),
        prompt: &task.prompt,
        events: timeline.to_vec(),
        output,
        diff,
    };
    serde_json::to_string_pretty(&payload).context("Failed to serialize export")
}
fn format_event_timeline(events: &[TaskEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| {
            format!(
                "- [{}] {}: {}",
                event.timestamp.format("%H:%M:%S"),
                event.event_kind.as_str(),
                event.full_detail(),
            )
        })
        .collect()
}
fn format_duration(duration_ms: Option<i64>) -> String {
    if let Some(ms) = duration_ms {
        let secs = ms / 1000;
        if secs < 60 {
            format!("{}s", secs)
        } else {
            format!("{}m {:02}s", secs / 60, secs % 60)
        }
    } else {
        "n/a".to_string()
    }
}
fn format_cost(cost: Option<f64>) -> String {
    cost
        .map(|value| format!("${value:.2}"))
        .unwrap_or_else(|| "n/a".to_string())
}
fn read_output(task: &Task) -> Result<String> {
    if let Some(ref path) = task.output_path {
        if !path.is_empty() {
            return fs::read_to_string(path)
                .with_context(|| format!("Failed to read output file {path}"));
        }
    }
    let persisted = paths::task_dir(task.id.as_str()).join("result.md");
    if persisted.exists() {
        return fs::read_to_string(&persisted)
            .with_context(|| format!("Failed to read result file {}", persisted.display()));
    }
    Ok(String::new())
}
fn load_task(store: &Store, task_id: &str) -> Result<Task> {
    store
        .get_task(task_id)?
        .ok_or_else(|| anyhow!("Task '{task_id}' not found"))
}

#[derive(Serialize)]
struct ExportPayload<'a> {
    id: &'a str,
    agent: &'a str,
    status: &'a str,
    duration: String,
    cost: String,
    created_at: String,
    prompt: &'a str,
    events: Vec<String>,
    output: &'a str,
    diff: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;
    use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    fn task(id: &str) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".to_string(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            worktree_path: None,
            worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            requested_model: None, observed_model: None, attribution_source: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn read_output_uses_persisted_result_when_output_path_absent() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp.path());
        let result_path = paths::task_dir("t-export-result").join("result.md");
        std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
        std::fs::write(&result_path, "exported report").unwrap();

        let output = read_output(&task("t-export-result")).unwrap();

        assert_eq!(output, "exported report");
    }
}
