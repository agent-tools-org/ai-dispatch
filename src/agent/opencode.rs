// OpenCode CLI adapter: builds `opencode run` commands, parses streaming output.
// OpenCode supports --format json for JSONL event streaming.

use anyhow::Result;
use chrono::Local;
use std::process::Command;

use super::RunOpts;
use crate::types::*;

pub struct OpenCodeAgent;

impl super::Agent for OpenCodeAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::OpenCode
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("opencode");
        cmd.arg("run");
        if let Some(ref model) = opts.model {
            cmd.args(["-m", model]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.args(["--dir", dir]);
        }
        cmd.arg(prompt);
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let now = Local::now();

        // OpenCode outputs plain text lines — classify by content patterns
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Try JSON parsing first (opencode may emit structured output)
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return parse_json_event(task_id, &v, now);
        }

        // Plain text classification
        let (kind, detail) = classify_text_line(trimmed);
        kind.map(|k| TaskEvent {
            task_id: task_id.clone(),
            timestamp: now,
            event_kind: k,
            detail: truncate(detail, 80),
            metadata: None,
        })
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        // Try to extract token count from output
        let tokens = extract_tokens_from_output(output);
        CompletionInfo { tokens, status: TaskStatus::Done, model: None, cost_usd: None }
    }
}

fn parse_json_event(
    task_id: &TaskId,
    v: &serde_json::Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type").and_then(|t| t.as_str())?;
    let detail = match event_type {
        "tool_call" | "function_call" => {
            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let args = v.get("arguments").and_then(|a| a.as_str()).unwrap_or("");
            format!("{name}: {}", truncate(args, 60))
        }
        "message" | "text" => {
            v.get("content").or(v.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string()
        }
        "completion" | "done" => {
            let tokens = v.get("tokens").and_then(|t| t.as_i64());
            match tokens {
                Some(t) => format!("completed with {} tokens", t),
                None => "completed".to_string(),
            }
        }
        _ => return None,
    };

    if detail.is_empty() {
        return None;
    }

    let event_kind = match event_type {
        "tool_call" | "function_call" => classify_tool_detail(&detail),
        "message" | "text" => EventKind::Reasoning,
        "completion" | "done" => EventKind::Completion,
        _ => EventKind::Reasoning,
    };

    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail: truncate(&detail, 80),
        metadata: None,
    })
}

fn classify_text_line(line: &str) -> (Option<EventKind>, &str) {
    if line.contains("error[") || line.contains("FAILED") || line.starts_with("Error:") {
        (Some(EventKind::Error), line)
    } else if line.contains("test result:") || line.contains("running") && line.contains("test") {
        (Some(EventKind::Test), line)
    } else if line.contains("Compiling") || line.contains("Finished") {
        (Some(EventKind::Build), line)
    } else if line.contains("git commit") || line.starts_with("commit ") {
        (Some(EventKind::Commit), line)
    } else if line.starts_with("Writing") || line.starts_with("Creating") {
        (Some(EventKind::FileWrite), line)
    } else if line.starts_with("Reading") {
        (Some(EventKind::FileRead), line)
    } else {
        // Skip noisy lines, keep substantive ones
        if line.len() > 10 {
            (Some(EventKind::Reasoning), line)
        } else {
            (None, line)
        }
    }
}

fn classify_tool_detail(detail: &str) -> EventKind {
    if detail.contains("cargo test") || detail.contains("npm test") {
        EventKind::Test
    } else if detail.contains("cargo build") || detail.contains("cargo check") {
        EventKind::Build
    } else if detail.contains("git commit") {
        EventKind::Commit
    } else {
        EventKind::ToolCall
    }
}

fn extract_tokens_from_output(output: &str) -> Option<i64> {
    // Look for common token reporting patterns
    for line in output.lines().rev() {
        if let Some(pos) = line.find("tokens:") {
            let after = &line[pos + 7..];
            if let Some(num) = after.trim().split_whitespace().next() {
                if let Ok(n) = num.replace(',', "").parse::<i64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.len() <= max {
        s
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}
