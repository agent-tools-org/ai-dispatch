// Tests for the Copilot CLI adapter covering command construction and JSON event parsing.
// Exports: module-scoped tests only.
// Deps: super::CopilotAgent, crate::agent::Agent, crate::types.

use super::CopilotAgent;
use crate::agent::{Agent, RunOpts};
use crate::types::{EventKind, TaskId};

fn opts() -> RunOpts {
    RunOpts {
        dir: Some("/tmp/project".to_string()),
        output: None,
        result_file: None,
        model: Some("gpt-5.2".to_string()),
        budget: false,
        read_only: false,
        context_files: vec!["/tmp/project/docs/spec.md".to_string()],
        session_id: None,
        env: None,
        env_forward: None,
    }
}

#[test]
fn build_command_uses_copilot_prompt_mode() {
    let cmd = CopilotAgent.build_command("ship it", &opts()).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(cmd.get_program().to_string_lossy(), "copilot");
    assert!(args.windows(2).any(|pair| pair == ["-p", "ship it"]));
    assert!(args.windows(2).any(|pair| pair == ["--output-format", "json"]));
    assert!(args.windows(2).any(|pair| pair == ["--stream", "on"]));
    assert!(args.windows(2).any(|pair| pair == ["--model", "gpt-5.2"]));
    assert!(args.windows(2).any(|pair| pair == ["--add-dir", "/tmp/project"]));
    assert!(args.windows(2).any(|pair| pair == ["--add-dir", "/tmp/project/docs"]));
}

#[test]
fn parse_event_reads_final_assistant_message() {
    let line = r#"{"type":"assistant.message","data":{"content":"hello","outputTokens":3}}"#;
    let event = CopilotAgent
        .parse_event(&TaskId("t-copilot".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert_eq!(event.detail, "hello");
}

#[test]
fn parse_event_reads_tool_execution_start() {
    let line =
        r#"{"type":"tool.execution_start","data":{"toolName":"view","arguments":{"path":"Cargo.toml"}}}"#;
    let event = CopilotAgent
        .parse_event(&TaskId("t-copilot".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::FileRead);
    assert_eq!(event.detail, "view: Cargo.toml");
}

#[test]
fn parse_event_ignores_persistence_session_errors() {
    let line =
        r#"{"type":"session.error","data":{"errorType":"persistence","message":"mkdir failed"}}"#;
    assert!(CopilotAgent
        .parse_event(&TaskId("t-copilot".to_string()), line)
        .is_none());
}
