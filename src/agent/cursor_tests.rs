// Cursor adapter tests for stream parsing, command construction, and rate-limit state.
// Exercises tool-call metadata variants and public Agent behavior.

use super::CursorAgent;
use crate::agent::{Agent, RunOpts};
use crate::types::{AgentKind, EventKind, TaskId};
use crate::{paths, rate_limit};
use std::fs;
use std::sync::{Mutex, OnceLock};

#[test]
fn parses_result_event_with_usage() {
    let event = parse(r#"{"type":"result","usage":{"inputTokens":3,"outputTokens":5,"cacheReadTokens":12260}}"#);
    assert_eq!(event.event_kind, EventKind::Completion);
    assert_eq!(event.detail, "tokens: 3 in + 5 out = 12268 (12260 cached)");
    assert_eq!(metadata_i64(&event, "tokens"), Some(12268));
    assert_eq!(metadata_i64(&event, "input_tokens"), Some(3));
    assert_eq!(metadata_i64(&event, "output_tokens"), Some(5));
}

#[test]
fn extracts_model_from_system_event() {
    let event = parse(r#"{"type":"system","subtype":"init","model":"composer-1.5"}"#);
    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert_eq!(event.detail, "init: composer-1.5");
    assert_eq!(metadata_str(&event, "model"), Some("composer-1.5"));
}

#[test]
fn parses_assistant_message() {
    let event = parse(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}]}}"#);
    assert_eq!(event.event_kind, EventKind::Reasoning);
    assert_eq!(event.detail, "Hello!");
    assert!(event.metadata.is_none());
}

#[test]
fn tool_call_ignores_sibling_metadata_and_preserves_raw_keys() {
    let cases = [
        ("readToolCall", "path", "src/read.rs", EventKind::FileRead, "completed: read", "files"),
        ("writeToolCall", "filePath", "src/write.rs", EventKind::FileWrite, "completed: write", "files"),
        ("editToolCall", "path", "src/edit.rs", EventKind::FileWrite, "completed: edit", "files"),
        ("deleteToolCall", "path", "src/delete.rs", EventKind::FileWrite, "completed: delete", "files"),
    ];
    for (tool, path_key, path, kind, detail, metadata_key) in cases {
        let line = format!(
            r#"{{"type":"tool_call","subtype":"completed","tool_call":{{"completedAtMs":"2","hookAdditionalContexts":[],"{tool}":{{"args":{{"{path_key}":"{path}"}},"result":{{"success":true}}}},"startedAtMs":"1","toolCallId":"id"}}}}"#
        );
        let event = parse(&line);
        assert_eq!(event.event_kind, kind);
        assert_eq!(event.detail, format!("{detail} {path}"));
        assert_eq!(metadata_first_str(&event, metadata_key), Some(path));
    }
}

#[test]
fn shell_tool_call_ignores_sibling_metadata_and_preserves_command() {
    let event = parse(r#"{"type":"tool_call","subtype":"started","tool_call":{"hookAdditionalContexts":[],"shellToolCall":{"args":{"command":"cargo check"}},"startedAtMs":"1","toolCallId":"id"}}"#);
    assert_eq!(event.event_kind, EventKind::ToolCall);
    assert_eq!(event.detail, "started: shell cargo check");
    assert_eq!(metadata_str(&event, "command"), Some("cargo check"));
}

#[test]
fn parses_tool_call_glob_as_tool_evidence() {
    let event = parse(r#"{"type":"tool_call","subtype":"started","tool_call":{"globToolCall":{"args":{"globPattern":"**/*.rs"}}}}"#);
    assert_eq!(event.event_kind, EventKind::ToolCall);
    assert_eq!(event.detail, "started: glob **/*.rs");
}

#[test]
fn skips_all_thinking_deltas() {
    let line = r#"{"type":"thinking","subtype":"delta","text":"analyzing the code"}"#;
    assert!(CursorAgent.parse_event(&TaskId("t-think".to_string()), line).is_none());
}

#[test]
fn uses_cursor_agent_binary() {
    let cmd = CursorAgent.build_command("test prompt", &run_opts()).unwrap();
    assert!(cmd.get_program() == "agent" || cmd.get_program() == "cursor-agent");
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args[0], "-p");
    assert!(args.windows(2).any(|window| window[0] == "--model" && window[1] == "composer-2"));
}

#[test]
fn build_command_embeds_context_files_in_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let context_file = dir.path().join("context.txt");
    fs::write(&context_file, "cursor context").unwrap();
    let mut opts = run_opts();
    opts.context_files = vec![context_file.to_string_lossy().into_owned()];

    let cmd = CursorAgent.build_command("test prompt", &opts).unwrap();
    let prompt = cmd.get_args().nth(1).unwrap().to_string_lossy();
    assert!(prompt.contains("test prompt"));
    assert!(prompt.contains("cursor context"));
}

#[test]
fn read_only_build_command_adds_trust_and_context() {
    let dir = tempfile::tempdir().unwrap();
    let context_file = dir.path().join("readonly.txt");
    fs::write(&context_file, "readonly context").unwrap();
    let mut opts = run_opts();
    opts.read_only = true;
    opts.context_files = vec![context_file.to_string_lossy().into_owned()];

    let cmd = CursorAgent.build_command("plan prompt", &opts).unwrap();
    let args: Vec<_> = cmd.get_args().collect();
    assert_eq!(args[1], "--trust");
    assert!(args.windows(2).any(|window| window[0] == "--mode" && window[1] == "plan"));
}

#[test]
fn parse_event_marks_plain_text_rate_limits() {
    assert_rate_limit("Error: rate limit exceeded, try again later", false);
}

#[test]
fn parse_event_marks_json_rate_limits() {
    assert_rate_limit(r#"{"type":"error","message":"quota exceeded for this workspace"}"#, true);
}

fn parse(line: &str) -> crate::types::TaskEvent {
    CursorAgent.parse_event(&TaskId("t-cursor".to_string()), line).unwrap()
}

fn metadata_i64(event: &crate::types::TaskEvent, key: &str) -> Option<i64> {
    event.metadata.as_ref()?.get(key)?.as_i64()
}

fn metadata_str<'a>(event: &'a crate::types::TaskEvent, key: &str) -> Option<&'a str> {
    event.metadata.as_ref()?.get(key)?.as_str()
}

fn metadata_first_str<'a>(event: &'a crate::types::TaskEvent, key: &str) -> Option<&'a str> {
    event.metadata.as_ref()?.get(key)?.as_array()?.first()?.as_str()
}

fn assert_rate_limit(line: &str, is_json: bool) {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let _guard = rate_limit_lock().lock().unwrap();
    let _ = rate_limit::clear_rate_limit(&AgentKind::Cursor);
    let event = parse(line);
    let expected = if is_json { "quota exceeded for this workspace" } else { line };
    assert_eq!(event.event_kind, EventKind::Error);
    assert_eq!(rate_limit::get_rate_limit_info(&AgentKind::Cursor).and_then(|info| info.message), Some(expected.to_string()));
    let _ = rate_limit::clear_rate_limit(&AgentKind::Cursor);
}

fn rate_limit_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn run_opts() -> RunOpts {
    RunOpts {
        dir: None, output: None, result_file: None, model: None, budget: false,
        read_only: false, sandbox: false, context_files: vec![], session_id: None,
        env: None, env_forward: None,
    }
}
