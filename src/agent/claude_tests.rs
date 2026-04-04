// Tests for the Claude CLI adapter covering command construction and stream parsing.
// Exports: module-scoped tests only.
// Deps: super::ClaudeAgent, crate::agent::Agent, tempfile.

use super::ClaudeAgent;
use crate::agent::{Agent, RunOpts};
use crate::types::{EventKind, TaskId};
use tempfile::tempdir;

#[test]
fn build_command_uses_stream_json_and_verbose() {
    let dir = tempdir().unwrap();
    let opts = RunOpts {
        dir: Some(dir.path().to_string_lossy().to_string()),
        output: None,
        model: Some("sonnet".to_string()),
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = ClaudeAgent.build_command("test prompt", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    assert_eq!(cmd.get_program().to_string_lossy(), "claude");
    assert!(args.windows(2).any(|pair| pair == ["--output-format", "stream-json"]));
    assert!(args.iter().any(|arg| arg == "--verbose"));
    assert!(args.windows(2).any(|pair| pair == ["--model", "sonnet"]));
    assert!(args.windows(2).any(|pair| pair[0] == "--add-dir"));
}

#[test]
fn build_command_read_only_restricts_tools() {
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
    let cmd = ClaudeAgent.build_command("inspect", &opts).unwrap();
    let args: Vec<String> = cmd.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
    assert!(args.windows(2).any(|pair| pair == ["--allowedTools", "Read,Glob,Grep,LS"]));
}

#[test]
fn parses_assistant_reasoning_event() {
    let line = r#"{"type":"assistant","message":{"model":"claude-sonnet-4-5","content":[{"type":"text","text":"Thinking through the change."}]},"session_id":"session-1"}"#;
    let event = ClaudeAgent
        .parse_event(&TaskId("t-claude".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert_eq!(event.detail, "Thinking through the change.");
    assert_eq!(
        event.metadata.as_ref().and_then(|value| value.get("model")).and_then(|value| value.as_str()),
        Some("claude-sonnet-4-5")
    );
}

#[test]
fn parses_tool_use_event() {
    let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"pwd","description":"Print working directory"}}]}}"#;
    let event = ClaudeAgent
        .parse_event(&TaskId("t-claude".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::ToolCall);
    assert_eq!(event.detail, "Bash: pwd");
}

#[test]
fn parses_completion_event() {
    let line = r#"{"type":"result","subtype":"success","result":"Hello!","total_cost_usd":0.14359275,"session_id":"session-1","usage":{"input_tokens":4,"cache_creation_input_tokens":18821,"cache_read_input_tokens":44733,"output_tokens":143},"modelUsage":{"claude-opus-4-6[1m]":{"inputTokens":4}}}"#;
        let event = ClaudeAgent
            .parse_event(&TaskId("t-claude".to_string()), line)
            .unwrap();
    assert_eq!(event.event_kind, EventKind::Completion);
    assert_eq!(
        event.metadata.as_ref().and_then(|value| value.get("tokens")).and_then(|value| value.as_i64()),
        Some(63_701)
    );
    assert_eq!(
        event.metadata.as_ref().and_then(|value| value.get("model")).and_then(|value| value.as_str()),
        Some("claude-opus-4-6")
    );
    assert_eq!(
        event.metadata.as_ref().and_then(|value| value.get("cost_usd")).and_then(|value| value.as_f64()),
        Some(0.14359275)
    );
}
