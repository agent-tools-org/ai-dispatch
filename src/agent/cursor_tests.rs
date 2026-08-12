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
fn prose_about_writing_is_not_a_cursor_file_event() {
    assert_ne!(
        super::classify_line("I wrote a report about the implementation.").0,
        Some(EventKind::FileWrite)
    );
    assert_eq!(
        super::classify_line("I wrote src/agent/cursor.rs").0,
        Some(EventKind::FileWrite)
    );
}

#[test]
fn tool_call_ignores_sibling_metadata_and_preserves_arguments() {
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
fn distinct_unknown_tools_preserve_distinct_command_metadata() {
    let first = parse(r#"{"type":"tool_call","subtype":"started","tool_call":{"alphaToolCall":{"args":{"query":"one"}}}}"#);
    let second = parse(r#"{"type":"tool_call","subtype":"started","tool_call":{"betaToolCall":{"args":{"query":"two"}}}}"#);

    assert_eq!(metadata_str(&first, "command"), Some("alphaToolCall:{\"query\":\"one\"}"));
    assert_eq!(metadata_str(&second, "command"), Some("betaToolCall:{\"query\":\"two\"}"));
    assert_ne!(metadata_str(&first, "command"), metadata_str(&second, "command"));
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
    // composer-2 was delisted by 2026-08-05; `cursor-agent models` marks
    // composer-2.5 as current. This assertion guarded the dead name for as long
    // as it was green.
    assert!(args.windows(2).any(|window| window[0] == "--model" && window[1] == "composer-2.5"));
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
fn read_only_with_result_file_uses_force_not_plan() {
    let mut opts = run_opts();
    opts.read_only = true;
    opts.result_file = Some("result.md".to_string());
    let cmd = CursorAgent.build_command("audit findings", &opts).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(args.iter().any(|a| a == "--force"));
    assert!(!args.windows(2).any(|w| w == ["--mode", "plan"]));
    assert!(args.iter().any(|a| a.contains("EXCEPT the result file")));
}

/// The adapter classifies; it does not decide who spoke. Every line below is one
/// cursor really produced on 2026-08-07, and each of the first three wrote a
/// real hold on a route that was serving the whole time — one of them permanent.
/// Detection now happens where the channel is known (`quota_channel`), so
/// parsing an event must leave the marker directory untouched whatever the line
/// says.
#[test]
fn parse_event_classifies_but_never_marks() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let _guard = rate_limit_lock().lock().unwrap();
    let _ = rate_limit::clear_rate_limit(&AgentKind::Cursor, None);

    let lines = [
        // The audit's own report, quoting this repo's fixture back at us.
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"FAILED: the needle is `quota exceeded for this workspace`"}]}}"#,
        // The audit's own grep, rendered by aid as `completed: grep <pattern>`.
        r#"{"type":"tool_call","subtype":"completed","tool_call":{"grepToolCall":{"args":{"pattern":"you're out of usage|out of usage|ActionRequired"}}}}"#,
        // A plain-text line carrying a generic token.
        "Error: rate limit exceeded, try again later",
        // And cursor's genuine refusal envelope — still not the adapter's call.
        r#"{"type":"error","message":"quota exceeded for this workspace"}"#,
    ];
    for line in lines {
        let _ = CursorAgent.parse_event(&TaskId("t-cursor".to_string()), line);
        assert!(
            !rate_limit::is_rate_limited(&AgentKind::Cursor, None),
            "adapter wrote a marker from {line:?}"
        );
        assert!(
            !rate_limit::is_group_rate_limited(&AgentKind::Cursor, None, "premium"),
            "adapter wrote a group marker from {line:?}"
        );
    }
}

/// The genuine refusal above is not lost — it is read on the channel it arrived
/// on, and it marks the premium tier rather than the whole agent, so `auto`
/// stays dispatchable.
#[test]
fn cursors_error_envelope_is_read_on_the_stream_channel() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let _guard = rate_limit_lock().lock().unwrap();
    let _ = rate_limit::clear_rate_limit(&AgentKind::Cursor, None);

    let line = r#"{"type":"error","message":"quota exceeded for this workspace"}"#;
    let refusal = rate_limit::refusal_on_channel(
        line,
        AgentKind::Cursor,
        crate::quota_channel::Channel::CliStream,
    );
    assert_eq!(refusal.as_deref(), Some("quota exceeded for this workspace"));
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

#[test]
fn only_accepts_a_bare_agent_binary_that_identifies_as_cursor() {
    // Cursor's own help text names the product; xAI's Grok Build CLI ships a binary with
    // the same `agent` name and must not be mistaken for it.
    assert!(super::help_mentions_cursor(
        "Usage: cursor-agent [OPTIONS]\n  -p, --print  Print response\n"
    ));
    assert!(super::help_mentions_cursor("Cursor Agent CLI\n"));
    assert!(!super::help_mentions_cursor(
        "Grok Build TUI\n\nUsage: agent [OPTIONS] [PROMPT] [COMMAND]\n"
    ));
    assert!(!super::help_mentions_cursor(""));
}

#[test]
fn parse_cursor_models_output_strips_ansi_escapes() {
    let raw = "\u{1b}[1mcomposer-2.5\u{1b}[0m (current)\n\u{1b}[32mcomposer-2.5-fast\u{1b}[0m\n\u{1b}[34mgpt-5.6\u{1b}[0m\n";
    let models = super::parse_cursor_models_output(raw);
    assert_eq!(models, vec!["composer-2.5", "composer-2.5-fast", "gpt-5.6"]);
}
