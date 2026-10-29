// Tests for `aid show` mode-specific rendering.
// Covers events-only and full-vs-truncated output selection.

use super::*;
use crate::store::Store;
use crate::types::{EventKind, Task, TaskEvent, TaskId, TaskStatus};
use chrono::Local;
use serde_json::json;
use std::sync::Arc;

fn task(id: &str) -> Task {
    crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Done)
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

#[test]
fn events_text_lists_only_events() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = task("t-events-only");
    store.insert_task(&task).unwrap();
    store
        .insert_event(&TaskEvent::new(task.id.clone(), EventKind::Milestone, "started work".to_string()))
        .unwrap();

    let text = events_text(&store, task.id.as_str(), false).unwrap();

    assert!(text.starts_with("Events:\n"));
    assert!(text.contains("[ milestone] started work"));
    assert!(!text.contains("Task:"));
}

#[test]
fn events_text_full_renders_metadata_full_detail() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = task("t-events-full");
    store.insert_task(&task).unwrap();
    let full_text = format!("reasoning {}", "y".repeat(120));
    let (detail, metadata) = crate::agent::truncate::capped_detail(&full_text);
    store
        .insert_event(&TaskEvent::new(task.id.clone(), EventKind::Reasoning, detail).with_metadata(metadata))
        .unwrap();

    let brief = events_text(&store, task.id.as_str(), false).unwrap();
    assert!(!brief.contains(&full_text));
    assert!(brief.contains("..."));

    let full = events_text(&store, task.id.as_str(), true).unwrap();
    assert!(full.contains(&full_text));
}

#[test]
fn output_mode_uses_full_flag_for_untruncated_messages() {
    let temp = crate::support::test_env::TestHome::new();
    std::fs::create_dir_all(crate::support::paths::logs_dir()).unwrap();
    std::fs::write(crate::support::paths::log_path("t-show-full"), numbered_assistant_log(22, 500))
        .unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = task("t-show-full");
    task.worktree_path = Some(temp.path().display().to_string());
    store.insert_task(&task).unwrap();

    let brief = render_output_text(&store, "t-show-full", false, false).unwrap();
    let full = render_output_text(&store, "t-show-full", true, false).unwrap();

    assert!(brief.contains("[... 12 messages omitted ...]"));
    assert!(brief.contains("--output --full"));
    assert_eq!(full.split("\n---\n").count(), 22);
    assert!(!full.contains("[... "));
}

#[test]
fn log_mode_uses_full_flag_for_untruncated_log() {
    let _temp = crate::support::test_env::TestHome::new();
    std::fs::create_dir_all(crate::support::paths::logs_dir()).unwrap();
    let log = (0..220)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(crate::support::paths::log_path("t-log-full"), log).unwrap();

    let brief = render_log_text("t-log-full", false).unwrap();
    let full = render_log_text("t-log-full", true).unwrap();

    assert!(!brief.contains("line-000"));
    assert!(full.contains("line-000"));
    assert!(full.contains("line-219"));
}

#[test]
fn trimmed_events_text_explains_missing_output() {
    let store = std::sync::Arc::new(crate::store::Store::open_memory().unwrap());
    store.db().execute_batch("INSERT INTO tasks (id,agent,prompt,status,created_at)
        VALUES ('trimmed','codex','test','done','2020-01-01')").unwrap();
    store.insert_event(&crate::types::TaskEvent::new(
        crate::types::TaskId("trimmed".into()),
        crate::types::EventKind::Build,
        "build".into(),
    ).with_metadata(Some(serde_json::json!({"full": {"trimmed": true, "bytes": 5000}})))).unwrap();
    for full in [false, true] {
        assert!(super::text::events_text(&store, "trimmed", full).unwrap().contains("output trimmed"));
    }
}
