// Oz (Warp) CLI adapter: builds `oz agent run` commands and parses streaming JSON events.
// Exports OzAgent for streaming runs.
// Depends on serde_json for event parsing.

use anyhow::Result;
use chrono::Local;
use std::process::Command;

use super::read_only::read_only_prompt;
use super::truncate::{capped_detail, capped_detail_with};
use super::RunOpts;
use crate::rate_limit;
use crate::types::*;

pub struct OzAgent;

fn parse_tool_call(v: &serde_json::Value) -> (EventKind, String, Option<serde_json::Value>) {
    let tool = v.get("tool").and_then(|t| t.as_str()).unwrap_or("tool");
    let title = v.get("title").and_then(|t| t.as_str()).unwrap_or(tool);
    let paths: Vec<&str> = v.get("file_paths").and_then(|p| p.as_array())
        .into_iter().flatten().filter_map(|p| p.as_str()).collect();
    let detail = match paths.is_empty() {
        true => title.to_string(),
        false => format!("{title}: {}", paths.join(", ")),
    };
    let metadata = (!paths.is_empty()).then(|| serde_json::json!({ "files": paths }));
    let kind = if tool == "edit_files" { EventKind::FileWrite } else { EventKind::ToolCall };
    let (detail, metadata) = capped_detail_with(&detail, metadata);
    (kind, detail, metadata)
}

impl super::Agent for OzAgent {
    fn kind(&self) -> AgentKind {
        AgentKind::Oz
    }

    fn streaming(&self) -> bool {
        true
    }

    fn build_command(&self, prompt: &str, opts: &RunOpts) -> Result<Command> {
        let prompt_with_ctx = super::embed_context_in_prompt(prompt, &opts.context_files)?;
        let effective_prompt = if opts.read_only {
            read_only_prompt(&prompt_with_ctx, opts)
        } else {
            prompt_with_ctx
        };
        let mut cmd = Command::new("oz");
        cmd.args(["agent", "run", "-p", &effective_prompt, "--output-format", "json"]);
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
                let (detail, metadata) = capped_detail(text);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Reasoning,
                    detail,
                    metadata,
                })
            }
            "tool_call" => {
                let (event_kind, detail, metadata) = parse_tool_call(&v);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind,
                    detail,
                    metadata,
                })
            }
            "error" => {
                let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
                if rate_limit::is_rate_limit_error(msg) {
                    rate_limit::mark_rate_limited(&crate::types::AgentKind::Oz, msg);
                }
                let (detail, metadata) = capped_detail(msg);
                Some(TaskEvent {
                    task_id: task_id.clone(),
                    timestamp: now,
                    event_kind: EventKind::Error,
                    detail,
                    metadata,
                })
            }
            "tool_result" | "system" => None,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OzAgent;
    use crate::agent::{Agent, RunOpts};
    use crate::{paths, rate_limit};
    use crate::types::{AgentKind, EventKind, TaskId};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn prompt_arg(cmd: &Command) -> String {
        let args: Vec<String> = cmd
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        let prompt_index = args
            .iter()
            .position(|arg| arg == "-p")
            .expect("prompt flag should exist");
        args[prompt_index + 1].clone()
    }

    fn write_temp_context_file(contents: &str) -> String {
        // pid+nanos alone collides when parallel tests hit the same clock tick;
        // the counter makes every call unique within the process.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir()
            .join(format!("oz-context-{}-{unique}-{seq}.txt", std::process::id()));
        std::fs::write(&path, contents).expect("context file should be written");
        path.to_string_lossy().to_string()
    }

    #[test]
    fn build_command_uses_oz() {
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
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
    fn build_command_embeds_context_files_into_prompt() {
        let context_file = write_temp_context_file("fn helper() {}\n");
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
            context_files: vec![context_file.clone()],
            session_id: None,
            env: None,
            env_forward: None,
        };

        let cmd = OzAgent.build_command("review this", &opts).expect("command should build");
        let prompt = prompt_arg(&cmd);

        assert!(prompt.contains("review this"));
        assert!(prompt.contains(&format!("[Context File: {}]", context_file)));
        assert!(prompt.contains("fn helper() {}"));

        let _ = std::fs::remove_file(context_file);
    }

    #[test]
    fn build_command_wraps_read_only_prompt() {
        let context_file = write_temp_context_file("const ANSWER: u32 = 42;\n");
        let opts = RunOpts {
            dir: None,
            output: None,
            result_file: Some("result.md".to_string()),
            model: None,
            budget: false,
            read_only: true,
            sandbox: false,
            context_files: vec![context_file.clone()],
            session_id: None,
            env: None,
            env_forward: None,
        };

        let cmd = OzAgent.build_command("inspect only", &opts).expect("command should build");
        let prompt = prompt_arg(&cmd);

        assert!(prompt.starts_with("IMPORTANT: READ-ONLY MODE."));
        assert!(prompt.contains("EXCEPT the result file specified in this prompt"));
        assert!(prompt.contains("inspect only"));
        assert!(prompt.contains(&format!("[Context File: {}]", context_file)));
        assert!(prompt.contains("const ANSWER: u32 = 42;"));

        let _ = std::fs::remove_file(context_file);
    }

    #[test]
    fn build_command_with_dir() {
        let opts = RunOpts {
            dir: Some("/tmp/test".to_string()),
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            sandbox: false,
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
        let agent = OzAgent;
        let cases = [
            (r#"{"type":"tool_call","tool":"edit_files","title":"Edit files","file_paths":["src/main.rs"]}"#, EventKind::FileWrite, "Edit files: src/main.rs", Some(1)),
            (r#"{"type":"tool_call","tool":"edit_files","title":"Edit files","file_paths":["src/main.rs","src/lib.rs"]}"#, EventKind::FileWrite, "Edit files: src/main.rs, src/lib.rs", Some(2)),
            (r#"{"type":"tool_call","tool":"search","title":"Search code"}"#, EventKind::ToolCall, "Search code", None),
        ];
        for (line, kind, detail, expected_file_count) in cases {
            let event = agent.parse_event(&TaskId("t-oz".to_string()), line).unwrap();
            assert_eq!(event.event_kind, kind);
            assert_eq!(event.detail, detail);
            let file_count = event.metadata.as_ref()
                .and_then(|metadata| metadata["files"].as_array()).map(Vec::len);
            assert_eq!(file_count, expected_file_count);
        }
    }

    #[test]
    fn parses_agent_reasoning_event() {
        let agent = OzAgent;
        let line = r#"{"type":"agent_reasoning","text":"Thinking about the problem..."}"#;
        let event = agent
            .parse_event(&TaskId("t-oz".to_string()), line)
            .unwrap();
        assert_eq!(event.event_kind, EventKind::Reasoning);
        assert_eq!(event.detail, "Thinking about the problem...");
    }

    #[test]
    fn parses_rate_limit_error_and_marks_agent() {
        let temp = tempfile::tempdir().unwrap();
        let _aid_home = paths::AidHomeGuard::set(temp.path());
        let _ = rate_limit::clear_rate_limit(&AgentKind::Oz);
        let agent = OzAgent;
        let line = r#"{"type":"error","message":"HTTP 429 too many requests"}"#;
        let event = agent
            .parse_event(&TaskId("t-oz".to_string()), line)
            .expect("error event should parse");

        assert_eq!(event.event_kind, EventKind::Error);
        let info =
            rate_limit::get_rate_limit_info(&AgentKind::Oz).expect("rate limit marker should be created");
        assert_eq!(info.message.as_deref(), Some("HTTP 429 too many requests"));

        let _ = rate_limit::clear_rate_limit(&AgentKind::Oz);
    }
}
