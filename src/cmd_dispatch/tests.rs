// Unit tests for command dispatch input resolution and run outcome reporting.
// Covers verification-aware exit codes and machine-parseable status tags.

use super::{
    RunExitStatus, background_status_line, exit_code_for_status, resolve_finding_content_from,
};
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use anyhow::Result;
use chrono::Local;
use std::io::{Cursor, Write};
use tempfile::NamedTempFile;

#[test]
fn resolve_finding_content_prefers_file() -> Result<()> {
    let mut file = NamedTempFile::new()?;
    write!(file, "from file")?;
    let mut stdin = Cursor::new("from stdin");
    let content = resolve_finding_content_from(
        Some("inline".to_string()), true,
        Some(file.path().to_string_lossy().into_owned()), false, &mut stdin,
    )?;
    assert_eq!(content, "from file");
    Ok(())
}

#[test]
fn resolve_finding_content_reads_stdin_when_requested() -> Result<()> {
    let mut stdin = Cursor::new("from stdin");
    let content = resolve_finding_content_from(
        Some("inline".to_string()), true, None, true, &mut stdin,
    )?;
    assert_eq!(content, "from stdin");
    Ok(())
}

#[test]
fn resolve_finding_content_errors_when_piped_without_stdin_flag() {
    let mut stdin = Cursor::new("from pipe");
    let error = resolve_finding_content_from(None, false, None, false, &mut stdin).unwrap_err();
    assert!(error.to_string().contains("No finding content provided"));
}

#[test]
fn resolve_finding_content_uses_inline_arg() -> Result<()> {
    let mut stdin = Cursor::new("");
    let content = resolve_finding_content_from(
        Some("inline".to_string()), false, None, true, &mut stdin,
    )?;
    assert_eq!(content, "inline");
    Ok(())
}

#[test]
fn resolve_finding_content_errors_without_input() {
    let mut stdin = Cursor::new("");
    let error = resolve_finding_content_from(None, false, None, true, &mut stdin).unwrap_err();
    assert!(error.to_string().contains("No finding content provided"));
}

#[test]
fn terminal_task_statuses_map_to_process_exit_codes() {
    for status in TaskStatus::ALL.into_iter().filter(TaskStatus::is_terminal) {
        let expected = if status == TaskStatus::Done { 0 } else { 1 };
        assert_eq!(exit_code_for_status(status, VerifyStatus::Passed), expected, "status {status}");
    }
}

#[test]
fn done_task_with_failed_verification_exits_one_and_reports_reason() {
    let task = task_with_verify_status(VerifyStatus::Failed);
    let outcome = RunExitStatus::from_task(
        &task,
        Some("Failed during verification: cargo check".to_string()),
    );
    let line = outcome.summary_line();

    assert_eq!(outcome.exit_code(), 1);
    assert!(line.starts_with("[STATUS=VERIFY_FAILED]"));
    assert!(line.contains("completed but verification failed"));
    assert!(line.contains("Failed during verification: cargo check"));
    assert!(!line.starts_with("[STATUS=DONE]"));
}

#[test]
fn run_status_lines_have_distinct_prefix_tags_despite_prose_collisions() {
    let done = status_line(TaskStatus::Done, VerifyStatus::Passed, None);
    let failed = status_line(
        TaskStatus::Failed, VerifyStatus::Skipped, Some("build not done; retry still running"),
    );
    let verify_failed = status_line(
        TaskStatus::Done, VerifyStatus::Failed, Some("failed after agent was done"),
    );
    let background = background_status_line(
        &TaskId("t-bg01".to_string()), "codex", "fix failed task once done",
    );
    let lines = [
        ("[STATUS=DONE]", done),
        ("[STATUS=FAILED]", failed),
        ("[STATUS=VERIFY_FAILED]", verify_failed),
        ("[STATUS=BG_RUNNING]", background),
    ];

    for (index, (expected_tag, line)) in lines.iter().enumerate() {
        assert_eq!(line.split_whitespace().next(), Some(*expected_tag));
        for (other_index, (other_tag, _)) in lines.iter().enumerate() {
            if index != other_index {
                assert!(!line.starts_with(other_tag));
            }
        }
    }
}

fn status_line(
    status: TaskStatus,
    verify_status: VerifyStatus,
    reason: Option<&str>,
) -> String {
    RunExitStatus {
        task_id: TaskId("t-test".to_string()), status, verify_status,
        duration_ms: 2_500, reason: reason.map(str::to_string),
    }
    .summary_line()
}

fn task_with_verify_status(verify_status: VerifyStatus) -> Task {
    Task {
        id: TaskId("t-test".to_string()), agent: AgentKind::Codex,
        custom_agent_name: None, prompt: "test prompt".to_string(), resolved_prompt: None,
        category: None, status: TaskStatus::Done, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None,
        worktree_path: None, worktree_branch: None, final_head_sha: None, final_branch: None,
        start_sha: None, log_path: None, output_path: None, tokens: None, prompt_tokens: None,
        duration_ms: Some(2_500), requested_model: None, observed_model: None, cost_usd: None, exit_code: None,
        created_at: Local::now(), completed_at: None, verify: None, verify_status,
        pending_reason: None, read_only: false, budget: false, audit_verdict: None,
        audit_report_path: None, delivery_assessment: None,
    }
}
