// Oz (Warp) CLI adapter: builds `oz agent run` commands and parses streaming JSON events.
// Exports OzAgent for streaming runs.
// Depends on serde_json for event parsing.

use anyhow::Result;
use chrono::Local;
use std::process::Command;

use super::truncate::truncate_text;
use super::RunOpts;
use crate::types::*;

pub struct OzAgent;

impl super::Agent for OzAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Oz
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let mut cmd = Command::new("oz");
        cmd.args(["agent", "run", "-p", prompt, "--output-format", "json"]);
        if let Some(ref dir) = opts.dir {
            cmd.args(["-C", dir]);
            cmd.current_dir(dir);
        }
        if let Some(ref model) = opts.model {
            cmd.args(["--model", model]);
        }
        Ok(cmd)
    }

    fn parse_event(&self, task_id: &TaskId, line: &str) -> Option<TaskEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let now = Local::now();
        let event_type = v.get("type")?.as_str()?;
        match event_type {
            "agent_reasoning" | "agent" => {
                let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("");
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
            "tool_call" => {
                let tool = v.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::ToolCall,
                    detail: truncate_text(tool, 80),
                    metadata: None,
                })
            }
            "error" => {
                let msg = v
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Error,
                    detail: truncate_text(msg, 80),
                    metadata: None,
                })
            }
            "tool_result" | "system" => None,
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
    use super::OzAgent;
    use crate::agent::{Agent, RunOpts};

    #[test]
    fn build_command_uses_oz() {
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
        let cmd = OzAgent.build_command("test prompt", &opts).unwrap();
        assert_eq!(cmd.get_program().to_str().unwrap(), "oz");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"agent".to_string()));
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"json".to_string()));
    }

    #[test]
    fn build_command_with_dir() {
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
        let cmd = OzAgent.build_command("test", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"-C".to_string()));
        assert!(args.contains(&"/tmp/test".to_string()));
    }

    #[test]
    fn parses_tool_call_event() {
        use crate::types::{EventKind, TaskId};
        let agent = OzAgent;
        let line = r#"{"type":"tool_call","tool":"edit_files","title":"Edit files","file_paths":["src/main.rs"]}"#;
        let event = agent
            .parse_event(&TaskId("t-oz".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::ToolCall);
        assert_eq!(event.detail, "edit_files");
    }

    #[test]
    fn parses_agent_reasoning_event() {
        use crate::types::{EventKind, TaskId};
        let agent = OzAgent;
        let line = r#"{"type":"agent_reasoning","text":"Thinking about the problem..."}"#;
        let event = agent
            .parse_event(&TaskId("t-oz".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert_eq!(event.detail, "Thinking about the problem...");
    }
}
