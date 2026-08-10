// Command Code adapter contract tests for command construction and NDJSON parsing.
// Covers explicit success, failure, unknown result values, sessions, and usage.

use super::super::Agent;
use super::*;
use crate::agent::RunOpts;
use crate::types::{EventKind, TaskId, TaskStatus};

fn base_opts() -> RunOpts {
    RunOpts {
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
    }
}

fn args_of(prompt: &str, opts: &RunOpts) -> Vec<String> {
    CommandCodeAgent
        .build_command(prompt, opts)
        .expect("command should build")
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn build_command_write_mode_uses_yolo_and_skip_flags() {
    let args = args_of("ship it", &base_opts());
    assert_eq!(args.first().map(String::as_str), Some("-p"));
    assert!(args.contains(&"--output-format".to_string()));
    assert!(args.contains(&"json".to_string()));
    assert!(args.contains(&"--skip-onboarding".to_string()));
    assert!(args.contains(&"--no-auto-update".to_string()));
    assert!(args.contains(&"--yolo".to_string()));
    assert!(!args.contains(&"--permission-mode".to_string()));
}

#[test]
fn build_command_read_only_never_uses_yolo() {
    let opts = RunOpts {
        read_only: true,
        ..base_opts()
    };
    let args = args_of("inspect", &opts);
    assert!(args.windows(2).any(|pair| pair == ["--permission-mode", "plan"]));
    assert!(!args.contains(&"--yolo".to_string()));
}

#[test]
fn build_command_adds_model_session_and_dir() {
    let opts = RunOpts {
        model: Some("gpt-5.6-sol".to_string()),
        session_id: Some("74aaa8".to_string()),
        dir: Some("/tmp".to_string()),
        ..base_opts()
    };
    let cmd = CommandCodeAgent
        .build_command("inspect", &opts)
        .expect("command should build");
    let args: Vec<String> = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5.6-sol"]));
    assert!(args.windows(2).any(|pair| pair == ["--session", "74aaa8"]));
    assert!(args.windows(2).any(|pair| pair == ["--add-dir", "/tmp"]));
    assert_eq!(cmd.get_current_dir(), Some(std::path::Path::new("/tmp")));
}

#[test]
fn parse_event_saves_session_id() {
    let event = CommandCodeAgent
        .parse_event(
            &TaskId("t-session".to_string()),
            r#"{"type":"event","event":{"type":"run_start","sessionId":"74aaa8c1-ae8d-45bc-ac19-ceebb870ff5a"}}"#,
        )
        .expect("event should parse");
    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(
        event
            .metadata
            .and_then(|meta| meta.get("agent_session_id").and_then(|value| value.as_str()).map(str::to_string))
            .as_deref(),
        Some("74aaa8c1-ae8d-45bc-ac19-ceebb870ff5a")
    );
}

#[test]
fn parse_event_captures_echoed_model() {
    let event = CommandCodeAgent
        .parse_event(
            &TaskId("t-model".to_string()),
            r#"{"type":"event","event":{"type":"model_request_start","model":"deepseek/deepseek-v4-flash"}}"#,
        )
        .expect("event should parse");
    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(
        event
            .metadata
            .and_then(|meta| meta.get("model").and_then(|value| value.as_str()).map(str::to_string))
            .as_deref(),
        Some("deepseek/deepseek-v4-flash")
    );
}

#[test]
fn parse_completion_marks_success_and_echoed_model() {
    let output = concat!(
        "{\"type\":\"event\",\"event\":{\"type\":\"run_start\",\"sessionId\":\"74aaa8\"}}\n",
        "{\"type\":\"event\",\"event\":{\"type\":\"model_request_start\",\"model\":\"deepseek/deepseek-v4-flash\"}}\n",
        "{\"type\":\"event\",\"event\":{\"type\":\"turn_end\",\"turnNumber\":1,\"hadToolCalls\":false,\"usage\":{\"inputTokens\":21863,\"outputTokens\":19,\"cacheReadTokens\":5760,\"cacheWriteTokens\":0}}}\n",
        "{\"type\":\"result\",\"subtype\":\"success\",\"sessionId\":\"74aaa8\",\"stopReason\":\"end_turn\",\"usage\":{\"inputTokens\":21863,\"outputTokens\":19,\"cacheReadTokens\":5760,\"cacheWriteTokens\":0},\"durationMs\":3617,\"finalText\":\"OK\"}\n"
    );
    let info = parse_commandcode_completion(output);
    assert_eq!(info.status, TaskStatus::Done);
    assert_eq!(info.model.as_deref(), Some("deepseek/deepseek-v4-flash"));
    assert_eq!(info.tokens, Some(21882));
}

#[test]
fn parse_completion_rejects_max_turns_result() {
    let output = concat!(
        "{\"type\":\"event\",\"event\":{\"type\":\"model_request_start\",\"model\":\"deepseek/deepseek-v4-flash\"}}\n",
        "{\"type\":\"result\",\"subtype\":\"max_turns\",\"sessionId\":\"b4d4fb\",\"stopReason\":\"max_turns\",\"usage\":{\"inputTokens\":21874,\"outputTokens\":129,\"cacheReadTokens\":5760,\"cacheWriteTokens\":0},\"durationMs\":4651,\"finalText\":\"\"}\n"
    );
    let info = parse_commandcode_completion(output);
    assert_eq!(info.status, TaskStatus::Failed);
    assert_eq!(info.model.as_deref(), Some("deepseek/deepseek-v4-flash"));
    assert_eq!(info.tokens, Some(22003));
}

#[test]
fn parse_completion_does_not_invent_failure_for_unknown_result_values() {
    let output = r#"{"type":"result","subtype":"future_success","stopReason":"new_terminal_reason","finalText":"ok"}"#;
    let info = parse_commandcode_completion(output);
    assert_eq!(info.status, TaskStatus::Done);
}

#[test]
fn parse_completion_rejects_explicit_error_flag() {
    let output = r#"{"type":"result","subtype":"future_value","is_error":true,"finalText":""}"#;
    let info = parse_commandcode_completion(output);
    assert_eq!(info.status, TaskStatus::Failed);
}

#[test]
fn parse_result_event_reports_real_failure_reason() {
    let event = CommandCodeAgent
        .parse_event(
            &TaskId("t-max-turns".to_string()),
            r#"{"type":"result","subtype":"max_turns","sessionId":"b4d4fb","stopReason":"max_turns","usage":{"inputTokens":21874,"outputTokens":129,"cacheReadTokens":5760,"cacheWriteTokens":0},"durationMs":4651,"finalText":""}"#,
        )
        .expect("result should parse");
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(event.detail.contains("max_turns"));
}
