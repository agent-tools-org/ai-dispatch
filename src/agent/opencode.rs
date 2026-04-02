// OpenCode CLI adapter: builds `opencode run` commands, parses streaming output.
// OpenCode supports --format json for JSONL event streaming.

use anyhow::Result;
use chrono::Local;
use serde_json::json;
use std::process::Command;

use super::truncate::truncate_text;
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
        if opts.read_only {
            aid_warn!("[aid] ⚠OpenCode read-only is prompt-level only, not enforced. Use --worktree for isolation.");
        }
        let effective_prompt = if opts.read_only {
            format!(
                "IMPORTANT: READ-ONLY MODE. Do NOT modify, create, or delete any files. Only read and analyze.\n\n{}",
                prompt
            )
        } else {
            prompt.to_string()
        };
        let mut cmd = Command::new("opencode");
        cmd.arg("run");
        cmd.args(["--format", "json"]);
        // Allow file access outside --dir (e.g. workgroup workspace symlinks)
        cmd.env(
            "OPENCODE_CONFIG_CONTENT",
            r#"{"agent":{"build":{"permission":{"external_directory":"allow"}}}}"#,
        );
        if let Some(ref session_id) = opts.session_id {
            cmd.args(["--session", session_id]);
            cmd.arg("--continue");
        }
        if let Some(ref model) = opts.model {
            cmd.args(["-m", model]);
        }
        if let Some(ref dir) = opts.dir {
            cmd.args(["--dir", dir]);
            cmd.current_dir(dir);
        }
        for file in &opts.context_files {
            cmd.args(["-f", file]);
        }
        cmd.arg(&effective_prompt);
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
            detail: truncate_text(detail, 80),
            metadata: None,
        })
    }

    fn needs_pty(&self) -> bool {
        true
    }

    fn parse_completion(&self, output: &str) -> CompletionInfo {
        let (tokens, cost_usd) = extract_tokens_from_output(output);
        CompletionInfo {
            tokens,
            status: TaskStatus::Done,
            model: None,
            cost_usd,
            exit_code: None,
        }
    }
}

pub(crate) fn parse_json_event(
    task_id: &TaskId,
    v: &serde_json::Value,
    now: chrono::DateTime<Local>,
) -> Option<TaskEvent> {
    let event_type = v.get("type").and_then(|t| t.as_str())?;
    let session_id = v.get("sessionID").and_then(|s| s.as_str());
    let (detail, metadata) = match event_type {
        "tool_call" | "function_call" => {
            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
            let args = v.get("arguments").and_then(|a| a.as_str()).unwrap_or("");
            (format!("{name}: {}", truncate_text(args, 60)), None)
        }
        "message" => {
            let detail = v
                .get("content")
                .or(v.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            (detail, None)
        }
        "text" => {
            let detail = v
                .pointer("/part/text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            (detail, None)
        }
        "step_start" => return None,
        "step_finish" => {
            let total = v.pointer("/part/tokens/total").and_then(|t| t.as_i64())?;
            let input = v.pointer("/part/tokens/input").and_then(|t| t.as_i64())?;
            let output = v.pointer("/part/tokens/output").and_then(|t| t.as_i64())?;
            let cost = v.pointer("/part/cost").and_then(|c| c.as_f64())?;
            (
                format!("tokens: {} in + {} out = {}", input, output, total),
                Some(json!({
                    "tokens": total,
                    "input_tokens": input,
                    "output_tokens": output,
                    "cost_usd": cost,
                })),
            )
        }
        "completion" | "done" => {
            let tokens = v.get("tokens").and_then(|t| t.as_i64());
            let detail = match tokens {
                Some(t) => format!("completed with {} tokens", t),
                None => "completed".to_string(),
            };
            let metadata = tokens.map(|value| json!({ "tokens": value }));
            (detail, metadata)
        }
        _ => return None,
    };

    if detail.is_empty() {
        return None;
    }

    let event_kind = match event_type {
        "tool_call" | "function_call" => classify_tool_detail(&detail),
        "message" | "text" => EventKind::Reasoning,
        "step_finish" | "completion" | "done" => EventKind::Completion,
        _ => EventKind::Reasoning,
    };

    let metadata = if let Some(sid) = session_id {
        match metadata {
            Some(mut m) => {
                if let Some(obj) = m.as_object_mut() {
                    obj.insert("agent_session_id".to_string(), json!(sid));
                }
                Some(m)
            }
            None => Some(json!({ "agent_session_id": sid })),
        }
    } else {
        metadata
    };

    Some(TaskEvent {
        task_id: task_id.clone(),
        timestamp: now,
        event_kind,
        detail: truncate_text(&detail, 80),
        metadata,
    })
}

pub(crate) fn classify_text_line(line: &str) -> (Option<EventKind>, &str) {
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

pub(crate) fn classify_tool_detail(detail: &str) -> EventKind {
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

pub(crate) fn extract_tokens_from_output(output: &str) -> (Option<i64>, Option<f64>) {
    let mut total_tokens: i64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut found_any = false;

    for line in output.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
            && v.get("type").and_then(|t| t.as_str()) == Some("step_finish")
            && let Some(part) = v.get("part")
        {
            if let Some(tokens) = part.get("tokens").and_then(|t| t.get("total"))
                && let Some(n) = tokens.as_i64()
            {
                total_tokens += n;
                found_any = true;
            }
            if let Some(cost) = part.get("cost").and_then(|c| c.as_f64()) {
                total_cost += cost;
            }
        }
    }

    if found_any {
        (Some(total_tokens), Some(total_cost))
    } else {
        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::super::Agent;
    use super::*;

    #[test]
    fn parses_step_finish_token_event() {
        let task_id = TaskId("t-step".to_string());
        let event = parse_json_event(
            &task_id,
            &serde_json::json!({
                "type": "step_finish",
                "part": {
                    "tokens": {
                        "total": 16125,
                        "input": 14040,
                        "output": 2,
                        "reasoning": 0
                    },
                    "cost": 0.0
                }
            }),
            Local::now(),
        )
        .unwrap();

        assert_eq!(event.event_kind, EventKind::Completion);
        assert_eq!(event.detail, "tokens: 14040 in + 2 out = 16125");
        assert_eq!(
            event.metadata,
            Some(serde_json::json!({
                "tokens": 16125,
                "input_tokens": 14040,
                "output_tokens": 2,
                "cost_usd": 0.0
            }))
        );
    }

    #[test]
    fn build_command_includes_file_flags_for_context_files() {
        let opts = RunOpts {
            dir: Some("/project".to_string()),
            output: None,
            model: Some("test-model".to_string()),
            budget: false,
            read_only: false,
            context_files: vec!["src/types.rs".to_string(), "src/lib.rs".to_string()],
            session_id: None,
            env: None,
            env_forward: None,
        };
        let cmd = OpenCodeAgent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|s: &std::ffi::OsStr| s.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"-f".to_string()));
        let f_indices: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-f")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(f_indices.len(), 2);
        assert_eq!(args[f_indices[0] + 1], "src/types.rs");
        assert_eq!(args[f_indices[1] + 1], "src/lib.rs");
        assert!(args.contains(&"test prompt".to_string()));
    }

    #[test]
    fn extracts_session_id_from_json_event() {
        let task_id = TaskId("t-sess".to_string());
        let event = parse_json_event(
            &task_id,
            &serde_json::json!({
                "type": "message",
                "content": "test",
                "sessionID": "ses_abc123"
            }),
            Local::now(),
        )
        .unwrap();

        assert_eq!(
            event
                .metadata
                .as_ref()
                .and_then(|m| m.get("agent_session_id").and_then(|s| s.as_str())),
            Some("ses_abc123")
        );
    }

    #[test]
    fn session_flags_appear_in_command() {
        let agent = OpenCodeAgent;
        let opts = RunOpts {
            dir: None,
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: Some("ses_test123".to_string()),
            env: None,
            env_forward: None,
        };
        let cmd = agent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"--session".to_string()));
        assert!(args.contains(&"ses_test123".to_string()));
        assert!(args.contains(&"--continue".to_string()));
    }

    #[test]
    fn opencode_needs_pty() {
        assert!(OpenCodeAgent.needs_pty());
    }

    #[test]
    fn codex_does_not_need_pty() {
        assert!(!super::super::codex::CodexAgent.needs_pty());
    }

    #[test]
    fn no_session_flags_when_session_id_absent() {
        let agent = OpenCodeAgent;
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
        let cmd = agent.build_command("test prompt", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(!args.contains(&"--session".to_string()));
        assert!(!args.contains(&"--continue".to_string()));
    }

    #[test]
    fn build_command_read_only_prepends_readonly_prefix() {
        let agent = OpenCodeAgent;
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
        let cmd = agent.build_command("analyze this code", &opts).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        let last_arg = args.last().expect("should have prompt as last arg");
        assert!(last_arg.contains("READ-ONLY MODE"));
        assert!(last_arg.starts_with("IMPORTANT: READ-ONLY MODE"));
        assert!(last_arg.contains("analyze this code"));
    }
}
