// Tests for summary conclusion extraction and missing output handling.
// Deps: parent extraction helpers, task fixtures, and tempfile.

use super::{extract_conclusion, extract_last_log_message};
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

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
        id: TaskId("t-summary-missing".into()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "test".into(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: Some(log.display().to_string()),
        output_path: Some("report.md".to_string()),
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: true,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
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
