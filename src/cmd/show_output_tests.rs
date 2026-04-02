// Tests for `aid show` output and diff helpers.
// Exports: none; validates the re-exported show_output module API.
// Deps: show_output hub, Store, Task, tempfile.
use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use tempfile::NamedTempFile;

fn test_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
    }
}

fn numbered_assistant_log(count: usize, body_len: usize) -> String {
    (0..count)
        .map(|index| {
            serde_json::to_string(&json!({
                "type": "message",
                "role": "assistant",
                "content": format!("message-{index:02}-{}", "x".repeat(body_len)),
            }))
        })
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
}

fn write_log(path: &Path, content: String) {
    std::fs::write(path, content).unwrap();
}

#[test]
fn reads_task_output_file() {
    let file = NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "hello\n").unwrap();
    let mut task = test_task("t-output");
    task.agent = AgentKind::Gemini;
    task.output_path = Some(file.path().display().to_string());
    assert_eq!(read_task_output(&task).unwrap(), "hello\n");
}

#[test]
fn tail_lines_keeps_only_requested_suffix() {
    assert_eq!(show_output_messages::tail_lines("a\nb\nc\nd", 2), "c\nd");
}

#[test]
fn parse_diff_stat_standard_line() {
    let entries = parse_diff_stat(" src/foo.rs | 8 +++++---\n");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["file"], json!("src/foo.rs"));
    assert_eq!(entry["insertions"], json!(5));
    assert_eq!(entry["deletions"], json!(3));
}

#[test]
fn parse_diff_stat_skips_binary_entries() {
    assert!(parse_diff_stat(" src/bin.dat | Bin 0 -> 123 bytes\n").is_empty());
}

#[test]
fn parse_diff_stat_empty_text() {
    assert!(parse_diff_stat("").is_empty());
}

#[test]
fn diff_text_falls_back_to_default_log_output() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
    std::fs::write(crate::paths::log_path("t-log-fallback"), "log output\n").unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let task = test_task("t-log-fallback");
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, "t-log-fallback").unwrap();

    assert!(text.contains("\n--- Output ---\nlog output\n"));
    assert!(!text.contains("no worktree diff or output file available"));
}

#[test]
fn extract_messages_from_log_collects_supported_formats() {
    let file = NamedTempFile::new().unwrap();
    let content = [
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "codex message" }
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "stream ",
            "delta": true
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "delta",
            "delta": true
        }),
        json!({
            "type": "text",
            "part": { "text": "opencode text part" }
        }),
        json!({
            "type": "text",
            "content": "gemini text event"
        }),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(file.path(), content).unwrap();

    let output = extract_messages_from_log(file.path(), false);

    assert_eq!(
        output,
        Some(
            "codex message\n---\nstream delta\n---\nopencode text part\n---\ngemini text event"
                .to_string()
        )
    );
}

#[test]
fn extract_messages_strips_ansi_before_parsing() {
    let file = NamedTempFile::new().unwrap();
    let esc = String::from(char::from(0x1b));
    let json1 = r#"{"type":"text","part":{"text":"hello from opencode"}}"#;
    let json2 = r#"{"type":"text","content":"clean line"}"#;
    let content = format!("{esc}[0m{esc}[32m{json1}{esc}[0m\n{json2}\n");
    std::fs::write(file.path(), content).unwrap();

    let output = extract_messages_from_log(file.path(), false);

    assert!(output.is_some());
    let output = output.unwrap();
    assert!(output.contains("hello from opencode"), "got: {output}");
    assert!(output.contains("clean line"), "got: {output}");
}

#[test]
fn extract_messages_collects_opencode_tool_use_errors() {
    let file = NamedTempFile::new().unwrap();
    let content = [
        json!({"type":"text","part":{"text":"I'll explore the workspace"}}),
        json!({"type":"tool_use","part":{"tool":"glob","state":{"status":"error","input":{"path":"/tmp/test"},"error":"Permission denied"}}}),
        json!({"type":"text","part":{"text":"Let me try another approach"}}),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(file.path(), content).unwrap();

    let output = extract_messages_from_log(file.path(), true).unwrap();

    assert!(output.contains("I'll explore the workspace"), "got: {output}");
    assert!(output.contains("[glob] Error: Permission denied"), "got: {output}");
    assert!(output.contains("Let me try another approach"), "got: {output}");
}

#[test]
fn extract_messages_from_log_returns_none_without_supported_messages() {
    let file = NamedTempFile::new().unwrap();
    std::fs::write(file.path(), "{\"type\":\"event\"}\nnot-json\n").unwrap();

    assert_eq!(extract_messages_from_log(file.path(), false), None);
}

#[test]
fn extract_messages_accumulates_cursor_assistant_deltas() {
    let log = "{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"composer-2\"}\n{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Hello \"}]}}\n{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"world!\"}]}}\n{\"type\":\"tool_call\",\"subtype\":\"started\",\"tool_call\":{\"readToolCall\":{\"args\":{\"filePath\":\"src/main.rs\"}}}}\n{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Done.\"}]}}";
    let file = NamedTempFile::new().unwrap();
    std::fs::write(file.path(), log).unwrap();
    let output = extract_messages_from_log(file.path(), true).unwrap();
    assert!(output.contains("Hello world!"), "Expected merged deltas, got: {output}");
    assert!(output.contains("Done."), "Expected separate message after tool_call");
}

#[test]
fn extract_messages_from_log_caps_message_count_and_size() {
    let file = NamedTempFile::new().unwrap();
    write_log(file.path(), numbered_assistant_log(22, 500));

    let output = extract_messages_from_log(file.path(), false).unwrap();
    let parts = output.split("\n---\n").collect::<Vec<_>>();

    assert_eq!(output.matches("\n---\n").count(), 10);
    assert_eq!(parts.len(), 11);
    assert!(parts[3].starts_with("[... 12 messages omitted ...]"));
    assert!(parts[0].starts_with("message-00-"));
    assert!(parts[10].starts_with("message-21-"));
    assert!(parts.iter().all(|part| part.len() <= 1_000));
    assert!(output.len() <= 8_000);
}

#[test]
fn extract_messages_full_skips_truncation() {
    let file = NamedTempFile::new().unwrap();
    write_log(file.path(), numbered_assistant_log(22, 500));

    let output = extract_messages_from_log(file.path(), true).unwrap();
    let parts: Vec<&str> = output.split("\n---\n").collect();

    assert_eq!(parts.len(), 22);
    assert!(parts[0].starts_with("message-00-"));
    assert!(parts[21].starts_with("message-21-"));
    assert!(!output.contains("[... "));
    assert!(parts.iter().all(|part| part.len() > 500));
}

#[test]
fn output_text_for_task_prefers_extracted_messages_to_raw_log() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
    let log_path = crate::paths::log_path("t-output-messages");
    let log_content = [
        json!({
            "type": "message",
            "role": "assistant",
            "content": "human-readable output"
        }),
        json!({
            "type": "text",
            "part": { "text": "second chunk" }
        }),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(&log_path, log_content).unwrap();

    let store = Store::open_memory().unwrap();
    let task = test_task("t-output-messages");
    store.insert_task(&task).unwrap();

    let output = output_text_for_task(&store, "t-output-messages", false).unwrap();

    assert_eq!(output, "human-readable output\n---\nsecond chunk");
}

#[test]
fn truncate_conclusion_at_2000_chars() {
    let short = "a".repeat(200);
    let short_file = NamedTempFile::new().unwrap();
    std::fs::write(short_file.path(), &short).unwrap();
    let mut short_task = test_task("t-summary-short");
    short_task.output_path = Some(short_file.path().display().to_string());
    assert_eq!(crate::cmd::summary::generate_summary(&short_task).conclusion, short);

    let long_file = NamedTempFile::new().unwrap();
    std::fs::write(long_file.path(), "b".repeat(2_500)).unwrap();
    let mut long_task = test_task("t-summary-long");
    long_task.output_path = Some(long_file.path().display().to_string());
    let conclusion = crate::cmd::summary::generate_summary(&long_task).conclusion;

    assert_eq!(conclusion.len(), 2_000);
    assert_eq!(conclusion, format!("{}...", "b".repeat(1_997)));
}

#[test]
fn extract_messages_research_relaxed_limits() {
    let file = NamedTempFile::new().unwrap();
    write_log(file.path(), numbered_assistant_log(15, 1_200));

    let output = show_output_messages::extract_messages_research(file.path()).unwrap();
    let parts = output.split("\n---\n").collect::<Vec<_>>();

    assert_eq!(parts.len(), 15);
    assert!(parts.iter().all(|part| part.len() > 1_000));
    assert!(output.contains("message-00-"));
    assert!(output.contains("message-14-"));
    assert!(!output.contains("[... "));
}

#[test]
fn output_text_uses_research_mode_for_no_worktree() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
    let log_path = crate::paths::log_path("t-research-output");
    write_log(&log_path, numbered_assistant_log(12, 1_200));

    let store = Arc::new(Store::open_memory().unwrap());
    let task = test_task("t-research-output");
    store.insert_task(&task).unwrap();

    let output = output_text(&store, "t-research-output").unwrap();
    let parts = output.split("\n---\n").collect::<Vec<_>>();

    assert_eq!(parts.len(), 12);
    assert!(parts.iter().all(|part| part.len() > 1_000));
    assert!(!output.contains("[... "));
}
