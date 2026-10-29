// Saved task output selection, extraction, and rendering regressions.
// Deps: parent show test fixtures.

use super::*;

#[test]
fn output_text_for_task_prefers_extracted_messages_to_raw_log() {
    let _temp = crate::support::test_env::TestHome::new();
    std::fs::create_dir_all(crate::support::paths::logs_dir()).unwrap();
    let log_path = crate::support::paths::log_path("t-output-messages");
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
    assert_eq!(
        crate::cmd::summary::generate_summary(&short_task).conclusion,
        short
    );

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
    let _temp = crate::support::test_env::TestHome::new();
    std::fs::create_dir_all(crate::support::paths::logs_dir()).unwrap();
    let log_path = crate::support::paths::log_path("t-research-output");
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

#[test]
fn extract_messages_from_log_parses_commandcode_event_wrapper() {
    let file = NamedTempFile::new().unwrap();
    let lines = concat!(
        "{\"type\":\"event\",\"event\":{\"type\":\"run_start\",\"sessionId\":\"sess-123\"}}\n",
        "{\"type\":\"event\",\"event\":{\"type\":\"thinking_delta\",\"delta\":\"Let me analyze\"}}\n",
        "{\"type\":\"event\",\"event\":{\"type\":\"message_update\",\"content\":[{\"type\":\"thinking\",\"thinking\":\"internal thought\"},{\"type\":\"text\",\"text\":\"Auditing the changes:\"}]}}\n",
        "{\"type\":\"event\",\"event\":{\"type\":\"tool_queued\",\"toolName\":\"shell_command\",\"input\":{\"command\":\"git diff\"}}}\n",
        "{\"type\":\"result\",\"subtype\":\"success\",\"sessionId\":\"sess-123\",\"finalText\":\"Audit completed: no issues found.\"}\n"
    );
    write_log(file.path(), lines.to_string());

    let output = extract_messages_from_log(file.path(), true, Some("commandcode")).unwrap();
    assert!(
        output.contains("Auditing the changes:"),
        "Output was: {output}"
    );
    assert!(output.contains("[shell_command]"), "Output was: {output}");
    assert!(
        output.contains("Audit completed: no issues found."),
        "Output was: {output}"
    );
    assert!(
        !output.contains("internal thought"),
        "Thinking should be dropped: {output}"
    );
    assert!(
        !output.contains("Let me analyze"),
        "Thinking deltas should be dropped: {output}"
    );
}

#[test]
fn extract_messages_from_log_returns_notice_for_unrecognized_json_stream() {
    let file = NamedTempFile::new().unwrap();
    let lines = concat!(
        "{\"unsupported_schema\":true,\"data\":\"line1\"}\n",
        "{\"unsupported_schema\":true,\"data\":\"line2\"}\n"
    );
    write_log(file.path(), lines.to_string());

    let output = extract_messages_from_log(file.path(), true, Some("custom_agent")).unwrap();
    assert!(
        output.contains("[Unrecognized JSON log format from custom_agent]"),
        "Output: {output}"
    );
    assert!(
        output.contains("Sample line: {\"unsupported_schema\":true"),
        "Output: {output}"
    );
    assert!(output.contains("See transcript at"), "Output: {output}");

    let output_unknown = extract_messages_from_log(file.path(), true, None).unwrap();
    assert!(
        output_unknown.contains("[Unrecognized JSON log format from unknown agent]"),
        "Output: {output_unknown}"
    );
}
