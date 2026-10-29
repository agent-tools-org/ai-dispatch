// Tests for summary conclusion extraction and missing output handling.
// Deps: parent extraction helpers, task fixtures, and tempfile.

use super::{extract_conclusion, extract_last_log_message};
use crate::types::{Task, TaskId, TaskStatus};

#[test]
fn extracts_copilot_message_after_tool_boundary() {
    let log = concat!(
        "{\"type\":\"assistant.message_delta\",\"data\":{\"deltaContent\":\"Inspecting \"}}\n",
        "{\"type\":\"assistant.message_delta\",\"data\":{\"deltaContent\":\"repo\"}}\n",
        "{\"type\":\"tool.execution_start\",\"data\":{\"toolName\":\"view\",\
        \"arguments\":{\"path\":\"Cargo.toml\"}}}\n",
        "{\"type\":\"assistant.message_delta\",\"data\":{\"deltaContent\":\"Done.\"}}\n",
        "{\"type\":\"assistant.message\",\"data\":{\"content\":\"Done.\"}}\n"
    );

    assert_eq!(extract_last_log_message(log).as_deref(), Some("Done."));
}

#[test]
fn extracts_text_from_content_arrays() {
    let log = concat!(
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":[",
        "{\"type\":\"text\",\"text\":\"Alpha\"},",
        "{\"type\":\"text\",\"text\":\" beta\"}]}\n"
    );

    assert_eq!(extract_last_log_message(log).as_deref(), Some("Alpha beta"));
}

fn missing_output_task(log: &std::path::Path) -> Task {
    Task {
        prompt: "test".into(),
        log_path: Some(log.display().to_string()),
        output_path: Some("report.md".to_string()),
        read_only: true,
        ..crate::types::Task::test_fixture(TaskId("t-summary-missing".into()), TaskStatus::Done)
    }
}

#[test]
fn extract_conclusion_reports_missing_owned_output_instead_of_log() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("task.jsonl");
    std::fs::write(
        &log,
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"LOG_CONCLUSION_NOT_A_REPORT\"}\n",
    )
    .unwrap();
    let task = missing_output_task(&log);

    let conclusion = extract_conclusion(&task);
    assert!(
        conclusion.contains("No task-owned output file"),
        "absence must be explicit: {conclusion}"
    );
    assert!(
        !conclusion.contains("LOG_CONCLUSION_NOT_A_REPORT"),
        "must not silently use the log as the conclusion: {conclusion}"
    );
}
