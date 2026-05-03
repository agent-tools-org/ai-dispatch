// Droid (Factory.ai) CLI adapter: builds `droid exec` commands and parses streaming JSON events.
// Exports DroidAgent for streaming runs.
// Depends on serde_json for event parsing.

use anyhow::Result;
use chrono::Local;
use serde_json::{Value, json};
use std::process::Command;

use super::truncate::truncate_text;
use super::RunOpts;
use crate::rate_limit;
use crate::types::*;

pub struct DroidAgent;

impl super::Agent for DroidAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Droid
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("droid");
        cmd.args(["exec", "--output-format", "stream-json"]);
        if opts.read_only {
            // `--use-spec` is droid's true read-only mode. `--auto low` still
            // permits file creation/modification in non-system directories,
            // so it is NOT a read-only mode despite the name.
            cmd.arg("--use-spec");
        } else {
            // `--auto high` still hits "insufficient permission to proceed.
            // Re-run with --skip-permissions-unsafe" on a wide range of
            // operations in headless aid runs. aid worktrees are sandboxed
            // by branch and the user has opted into autonomous orchestration
            // (parallel to `gemini -y` and `cursor --trust`), so adopt
            // droid's own recommendation. Note: --skip-permissions-unsafe
            // cannot be combined with --auto.
            cmd.arg("--skip-permissions-unsafe");
        }
        if let Some(ref model) = opts.model {
            let mapped = map_model_name(model);
            cmd.args(["-m", mapped.as_str()]);
        }
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["-s", session_id]);
        }
        // `-f` in droid means "read PROMPT from file" — using it for context
        // files would override the prompt argument. `--append-system-prompt-file`
        // is purpose-built for injecting extra context per file and is repeatable.
        for file in &opts.context_files {
            cmd.args(["--append-system-prompt-file", file]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.args(["--cwd", dir]);
            cmd.current_dir(dir);
        }
        cmd.arg(prompt);
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let v: Value = serde_json::from_str(line).ok()?;
        let now = Local::now();
        let event_type = v.get("type")?.as_str()?;
        match event_type {
            "assistant_message" | "text" => {
                let text = v
                    .get("content")
                    .or_else(|| v.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                if text.is_empty() {
                    return None;
                }
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Reasoning,
                    detail: truncate_text(text, 80),
                    metadata: None,
                })
            }
            // Only emit on the request side. droid stream-json fires both
            // `tool_call` (the model's request) and `tool_result` (the tool's
            // response) for one logical operation, plus sometimes `tool_use`
            // as an alias. Treating all three as ToolCall events doubled the
            // event count, so the LoopDetector tripped after ~5 legit reads
            // (10 events with detail "Read"). Keep only `tool_call`.
            "tool_call" => {
                let name = v
                    .get("toolName")
                    .or_else(|| v.get("toolId"))
                    .or_else(|| v.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool");
                let detail = truncate_text(name, 80);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::ToolCall,
                    detail,
                    metadata: None,
                })
            }
            "tool_use" | "tool_result" => None,
            "mission_step" => parse_mission_step(task_id, &v, now),
            "session_forked" => parse_session_forked(task_id, &v, now),
            "usage" | "turn_complete" => {
                let input = v.get("input_tokens").and_then(|t| t.as_i64()).unwrap_or(0);
                let output = v.get("output_tokens").and_then(|t| t.as_i64()).unwrap_or(0);
                let total = input + output;
                let cost = v.get("cost_usd").and_then(|c| c.as_f64());
                let model = v.get("model").and_then(|m| m.as_str()).map(ToOwned::to_owned);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Completion,
                    detail: format!("tokens: {input} in + {output} out = {total}"),
                    metadata: Some(json!({
                        "tokens": total, "input_tokens": input, "output_tokens": output,
                        "model": model, "cost_usd": cost,
                    })),
                })
            }
            "error" => parse_error_event(task_id, &v, now),
            _ => None,
        }
    }

    fn parse_completion(&self, _output: &str) -> CompletionInfo {
        CompletionInfo {
            tokens: None,
            status: TaskStatus::Done,
            model: None,
            cost_usd: None,
            exit_code: None,
        }
    }
}

fn map_model_name(model: &str) -> String {
    match model {
        "haiku" => "claude-haiku-4-5-20251001".to_string(),
        "sonnet" => "claude-sonnet-4-6".to_string(),
        // droid's own default is claude-opus-4-7 per `droid exec --help`.
        "opus" => "claude-opus-4-7".to_string(),
        "gpt-4.1-nano" => "gpt-5.4-mini".to_string(),
        "gpt-4.1-mini" => "gpt-5.4-fast".to_string(),
        other => other.to_string(),
    }
}

fn parse_mission_step(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let description = v.get("description")?.as_str()?.trim();
    if description.is_empty() {
        return None;
    }
    let detail = match v.get("step").and_then(|value| value.as_str()) {
        Some(step) if !step.is_empty() => truncate_text(&format!("{step} {description}"), 80),
        _ => truncate_text(description, 80),
    };
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail,
        metadata: None,
    })
}

fn parse_session_forked(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let new_id = v.get("new_id")?.as_str()?;
    let detail = match v.get("parent_id").and_then(|value| value.as_str()) {
        Some(parent_id) if !parent_id.is_empty() => format!("forked {new_id} from {parent_id}"),
        _ => format!("forked {new_id}"),
    };
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Milestone,
        detail,
        metadata: Some(json!({ "agent_session_id": new_id })),
    })
}

fn parse_error_event(
    task_id: &TaskId,
    v: &Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let detail = droid_error_detail(v);
    if is_droid_rate_limit(v, detail.as_deref()) {
        let rate_limit_message = detail.clone().unwrap_or_else(|| "status 429".to_string());
        rate_limit::mark_rate_limited(&AgentKind::Droid, &rate_limit_message);
    }
    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind: EventKind::Error,
        detail: truncate_text(detail.as_deref().unwrap_or("unknown error"), 80),
        metadata: None,
    })
}

fn droid_error_detail(v: &Value) -> Option<String> {
    let message = v
        .get("message")
        .and_then(|value| value.as_str())
        .or_else(|| v.get("error").and_then(|value| value.as_str()))
        .or_else(|| v.pointer("/error/message").and_then(|value| value.as_str()));
    if let Some(message) = message
        && !message.is_empty()
    {
        return Some(message.to_string());
    }
    v.get("error_type")
        .and_then(|value| value.as_str())
        .or_else(|| v.pointer("/error/type").and_then(|value| value.as_str()))
        .map(ToOwned::to_owned)
}

fn is_droid_rate_limit(v: &Value, detail: Option<&str>) -> bool {
    if detail.is_some_and(rate_limit::is_rate_limit_error) {
        return true;
    }
    let status = v
        .get("status")
        .and_then(|value| value.as_i64())
        .or_else(|| v.pointer("/error/status").and_then(|value| value.as_i64()));
    if status == Some(429) {
        return true;
    }
    v.get("error_type")
        .and_then(|value| value.as_str())
        .or_else(|| v.pointer("/error/type").and_then(|value| value.as_str()))
        .is_some_and(|value| value.eq_ignore_ascii_case("rate_limit_exceeded"))
}

#[cfg(test)]
mod model_name_tests {
    use super::map_model_name;

    #[test]
    fn maps_common_shorthand_models() {
        assert_eq!(map_model_name("haiku"), "claude-haiku-4-5-20251001");
        assert_eq!(map_model_name("sonnet"), "claude-sonnet-4-6");
        assert_eq!(map_model_name("opus"), "claude-opus-4-7");
        assert_eq!(map_model_name("gpt-4.1-nano"), "gpt-5.4-mini");
        assert_eq!(map_model_name("gpt-4.1-mini"), "gpt-5.4-fast");
    }

    #[test]
    fn preserves_full_model_ids() {
        assert_eq!(
            map_model_name("claude-haiku-4-5-20251001"),
            "claude-haiku-4-5-20251001"
        );
    }
}

#[cfg(test)]
mod tests;
