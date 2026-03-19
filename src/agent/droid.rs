// Droid (Factory.ai) CLI adapter: builds `droid exec` commands and parses streaming JSON events.
// Exports DroidAgent for streaming runs.
// Depends on serde_json for event parsing.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
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
            cmd.arg("--auto").arg("low");
        } else {
            cmd.arg("--auto").arg("medium");
        }
        if let Some(ref model) = opts.model {
            cmd.args(["-m", model]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.args(["--cwd", dir]);
            cmd.current_dir(dir);
        }
        cmd.arg(prompt);
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
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
            "tool_use" | "tool_result" | "tool_call" => {
                let name = v.get("toolName").or_else(|| v.get("toolId")).or_else(|| v.get("name")).and_then(|n| n.as_str()).unwrap_or("tool");
                let detail = truncate_text(name, 80);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::ToolCall,
                    detail,
                    metadata: None,
                })
            }
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
            "error" => {
                let msg = v
                    .get("message")
                    .or_else(|| v.get("error"))
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown error");
                if rate_limit::is_rate_limit_error(msg) {
                    rate_limit::mark_rate_limited(&AgentKind::Droid, msg);
                }
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Error,
                    detail: truncate_text(msg, 80),
                    metadata: None,
                })
            }
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

#[cfg(test)]
mod tests {
    use super::DroidAgent;
    use crate::agent::{Agent, RunOpts};

    #[test]
    fn build_command_uses_droid_exec() {
        let opts = RunOpts {
            dir: None,
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = DroidAgent.build_command("test prompt", &opts).unwrap();
        assert_eq!(cmd.get_program().to_str().unwrap(), "droid");
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"medium".to_string()));
    }

    #[test]
    fn build_command_read_only_uses_auto_low() {
        let opts = RunOpts {
            dir: None,
            output: None,
            model: None,
            budget: false,
            read_only: true,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = DroidAgent.build_command("test", &opts).unwrap();
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.contains(&"low".to_string()));
    }

    #[test]
    fn parses_tool_call_events_with_tool_name() {
        use crate::types::{EventKind, TaskId};
        let agent = DroidAgent;
        let line = r#"{"type":"tool_call","id":"toolu_01","toolId":"Read","toolName":"Read","parameters":{"file_path":"src/main.rs"}}"#;
        let event = agent.parse_event(&TaskId("t-droid".to_string()), line).unwrap();
        assert_eq!(event.event_kind, EventKind::ToolCall);
        assert_eq!(event.detail, "Read");
    }

    #[test]
    fn build_command_with_dir_sets_cwd() {
        let opts = RunOpts {
            dir: Some("/tmp/test".to_string()),
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = DroidAgent.build_command("test", &opts).unwrap();
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect();
        assert!(args.contains(&"--cwd".to_string()));
        assert!(args.contains(&"/tmp/test".to_string()));
    }
}
