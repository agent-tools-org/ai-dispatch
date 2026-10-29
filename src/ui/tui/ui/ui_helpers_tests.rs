// Tests for TUI task formatting and detail helpers.
// Exercises route labels, output loading, and failure reasons.

use super::*;
use crate::task::view::read_output;
use crate::types::{TaskId, VerifyStatus};
use chrono::Local;
use tempfile::NamedTempFile;

fn make_task() -> Task {
    Task {
        project_id: crate::config::project::current_project_id(),
        ..crate::types::Task::test_fixture(TaskId("t-ui".to_string()), TaskStatus::Done)
    }
}

#[test]
fn task_header_shows_route_triple_not_opaque_agent() {
    use crate::types::AttributionSource;
    let mut task = make_task();
    task.requested_model = Some("gpt-5.6".to_string());
    task.observed_model = Some("gpt-5.6".to_string());
    task.attribution_source = Some(AttributionSource::Echoed);
    // task_header is a Paragraph; assert the source string it is built from.
    assert_eq!(task.display_route(), "codex/openai-chatgpt-plan/gpt-5.6");
    let _ = task_header(&task, &[]);
}

#[test]
fn task_header_unknown_model_is_literal_unknown() {
    let task = make_task();
    assert!(task.display_route().ends_with("/unknown"));
    assert_eq!(task.display_model(), None);
    let _ = task_header(&task, &[]);
}

#[test]
fn status_label_shows_inconclusive_verification() {
    let mut task = make_task();
    task.verify = Some("cargo test".to_string());
    task.verify_status = VerifyStatus::TimedOut;

    assert_eq!(task_status_label(&task), "DONE [VTIMEOUT]");
}

#[test]
fn detail_output_prefers_output_file() {
    let output_file = NamedTempFile::new().unwrap();
    std::fs::write(output_file.path(), "stdout\n").unwrap();
    let mut task = make_task();
    task.output_path = Some(output_file.path().display().to_string());

    assert_eq!(read_output(&task), "stdout\n");
}

#[test]
fn last_error_detail_prefers_trigger_error_over_earlier_noise() {
    use crate::types::{EventKind, TaskEvent};
    let now = Local::now();
    let events = vec![
        TaskEvent::at(
            TaskId("t-trigger".to_string()),
            EventKind::Error,
            "bot.rs:41: priority_baseline_ha".to_string(),
            now,
        ),
        TaskEvent::at(
            TaskId("t-trigger".to_string()),
            EventKind::Error,
            "apply_patch verification failed: Failed to find expected lines".to_string(),
            now,
        ),
        TaskEvent::at(
            TaskId("t-trigger".to_string()),
            EventKind::Error,
            "Failed during verification: cargo check ...".to_string(),
            now,
        ),
    ];

    let reason = last_error_detail(&events).expect("expected a Reason");
    assert!(reason.contains("apply_patch"));
}

#[test]
fn last_error_detail_falls_back_to_first_error_without_trigger() {
    use crate::types::{EventKind, TaskEvent};
    let now = Local::now();
    let events = vec![
        TaskEvent::at(
            TaskId("t-fallback".to_string()),
            EventKind::Error,
            "first generic error".to_string(),
            now,
        ),
        TaskEvent::at(
            TaskId("t-fallback".to_string()),
            EventKind::Error,
            "second generic error".to_string(),
            now,
        ),
    ];

    assert_eq!(
        last_error_detail(&events).as_deref(),
        Some("first generic error")
    );
}

#[test]
fn detail_output_parses_log_jsonl_content() {
    let log_file = NamedTempFile::new().unwrap();
    std::fs::write(
        log_file.path(),
        concat!(
            "not-json-prefix\n",
            "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"hello\"}}\n",
            "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\" world\",\"delta\":true}\n"
        ),
    )
    .unwrap();
    let mut task = make_task();
    task.log_path = Some(log_file.path().display().to_string());

    assert_eq!(read_output(&task), "hello\n---\n world");
}
