// Tests for `aid show` mode-specific rendering.
// Covers events-only and full-vs-truncated output selection.

use super::*;
use crate::store::Store;
use crate::types::{AgentKind, EventKind, Task, TaskEvent, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use serde_json::json;
use std::sync::Arc;

fn task(id: &str) -> Task {
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
        repo_path: None, project_id: None,
        worktree_path: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
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

#[test]
fn events_text_lists_only_events() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = task("t-events-only");
    store.insert_task(&task).unwrap();
    store
        .insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Milestone,
            detail: "started work".to_string(),
            metadata: None,
        })
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
        .insert_event(&TaskEvent {
            task_id: task.id.clone(),
            timestamp: Local::now(),
            event_kind: EventKind::Reasoning,
            detail,
            metadata,
        })
        .unwrap();

    let brief = events_text(&store, task.id.as_str(), false).unwrap();
    assert!(!brief.contains(&full_text));
    assert!(brief.contains("..."));

    let full = events_text(&store, task.id.as_str(), true).unwrap();
    assert!(full.contains(&full_text));
}

#[test]
fn output_mode_uses_full_flag_for_untruncated_messages() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
    std::fs::write(crate::paths::log_path("t-show-full"), numbered_assistant_log(22, 500))
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
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
    let log = (0..220)
        .map(|index| format!("line-{index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(crate::paths::log_path("t-log-full"), log).unwrap();

    let brief = render_log_text("t-log-full", false).unwrap();
    let full = render_log_text("t-log-full", true).unwrap();

    assert!(!brief.contains("line-000"));
    assert!(full.contains("line-000"));
    assert!(full.contains("line-219"));
}
