// Tests for PTY awaiting-input prompt capture.
// Covers metadata written for board-facing AWAIT reasons.
// Depends on pty_watch helpers and the in-memory Store.

use super::{
    MonitorState, extract_awaiting_prompt, finalize_streaming, mark_awaiting_input,
};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn stores_recent_question_as_awaiting_prompt_metadata() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = Task {
        id: TaskId("t-pty1".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
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
        };
    store.insert_task(&task).unwrap();

    let prompt = "115:    use super::board::render_board;";
    let awaiting_prompt = extract_awaiting_prompt(
        "Should I update board.rs?\n115:    use super::board::render_board;",
        prompt,
    );
    let mut awaiting_input = false;
    mark_awaiting_input(
        &store,
        &task.id,
        prompt,
        &awaiting_prompt,
        &mut awaiting_input,
    )
    .unwrap();

    let event = store.get_events(task.id.as_str()).unwrap().pop().unwrap();
    assert_eq!(event.detail, prompt);
    assert_eq!(
        event
            .metadata
            .as_ref()
            .and_then(|m| m.get("awaiting_prompt"))
            .and_then(|v| v.as_str()),
        Some("Should I update board.rs?")
    );
}

#[test]
fn handles_ansi_escaped_output() {
    let input = "\x1b[1mShould I proceed?\x1b[0m";
    let prompt = "fallback";
    let result = extract_awaiting_prompt(input, prompt);
    assert_eq!(result, "Should I proceed?");
}

#[test]
fn finds_question_beyond_six_lines() {
    let mut output = String::new();
    for i in 0..15 {
        output.push_str(&format!("Line {}\n", i));
    }
    output.push_str("What about this file?\n");
    output.push_str("116:    use super::other;\n");

    let result = extract_awaiting_prompt(&output, "fallback");
    assert_eq!(result, "What about this file?");
}

#[test]
fn matches_patterns_without_question_mark() {
    let output = "Do you want to continue\n1: code here";
    let result = extract_awaiting_prompt(output, "fallback");
    assert_eq!(result, "Do you want to continue");
}

#[test]
fn falls_back_to_prompt_when_no_question() {
    let output = "Some random output\nNo question here\nJust code lines";
    let result = extract_awaiting_prompt(output, "fallback prompt");
    assert_eq!(result, "fallback prompt");
}

#[test]
fn handles_osc_escaped_output() {
    let input = "\x1b]0;⛬ window title\x07\x1b[1mShould I proceed?\x1b[0m";
    let result = extract_awaiting_prompt(input, "fallback");
    assert_eq!(result, "Should I proceed?");
}

#[test]
fn auto_nudge_echo_does_not_reset_idle_progress_clock() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-nudge-echo-idle", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    let idle = Duration::from_secs(2);
    let stale = Instant::now() - idle - Duration::from_millis(50);
    state.last_progress_time = stale;
    state.idle_nudged = true;
    let nudge = crate::pty_watch_idle::default_nudge_message();
    crate::pty_watch_idle::register_inbound_echo(&mut state.inbound_echo_suppress, nudge.clone());
    let mut log = tempfile::NamedTempFile::new().unwrap();

    // Stream goes silent except for aid's own nudge echoes (the live failure shape).
    state
        .handle_chunk(
            &crate::agent::opencode::OpenCodeAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            format!("{nudge}\n{nudge}\n"),
        )
        .unwrap();

    assert_eq!(state.event_count, 0);
    assert!(state.idle_nudged);
    assert_eq!(state.last_progress_time, stale);
    let would_hang = state.last_progress_time.elapsed() > idle;
    assert!(
        would_hang,
        "nudge-only stream must still be past idle timeout for hung_detected"
    );
    let events = store.get_events(task.id.as_str()).unwrap();
    assert!(
        events.iter().all(|event| event.detail != nudge),
        "nudge echo must not be stored as agent progress: {events:?}"
    );
    println!(
        "PROOF nudge-only: events={} idle_nudged={} elapsed_ms={} idle_ms={} would_hung_detected={}",
        state.event_count,
        state.idle_nudged,
        state.last_progress_time.elapsed().as_millis(),
        idle.as_millis(),
        would_hang
    );
}

#[test]
fn agent_resume_after_nudge_echo_does_reset_idle_progress_clock() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-nudge-then-resume", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    let idle = Duration::from_secs(2);
    state.last_progress_time = Instant::now() - idle - Duration::from_millis(50);
    state.idle_nudged = true;
    let nudge = crate::pty_watch_idle::default_nudge_message();
    crate::pty_watch_idle::register_inbound_echo(&mut state.inbound_echo_suppress, nudge.clone());
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::opencode::OpenCodeAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            format!(
                "{nudge}\n{}\n",
                r#"{"type":"tool_call","name":"bash","arguments":"ls"}"#
            ),
        )
        .unwrap();

    assert!(state.event_count >= 1);
    assert!(!state.idle_nudged);
    let would_hang = state.last_progress_time.elapsed() > idle;
    assert!(
        !would_hang,
        "agent resume after nudge must refresh progress and clear hung"
    );
    println!(
        "PROOF nudge-then-resume: events={} idle_nudged={} elapsed_ms={} idle_ms={} would_hung_detected={}",
        state.event_count,
        state.idle_nudged,
        state.last_progress_time.elapsed().as_millis(),
        idle.as_millis(),
        would_hang
    );
}

#[test]
fn reasoning_events_refresh_liveness_clock() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-opencode-reasoning-progress", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(10);
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::opencode::OpenCodeAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "Thinking through the next step...\n".to_string(),
        )
        .unwrap();

    assert_eq!(state.event_count, 1);
    assert!(state.last_progress_time.elapsed() < Duration::from_secs(5));
}

#[test]
fn milestone_output_refreshes_progress_clock() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-milestone-progress", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    state.last_progress_time = Instant::now() - Duration::from_secs(10);
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::codex::CodexAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "[MILESTONE] real progress\n".to_string(),
        )
        .unwrap();

    assert_eq!(state.event_count, 1);
    assert!(state.last_progress_time.elapsed() < Duration::from_secs(5));
    assert_eq!(store.get_events(task.id.as_str()).unwrap().len(), 1);
}

#[test]
fn finding_line_with_workgroup_records_shared_finding() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = pty_task("t-pty-finding", TaskStatus::Running);
    task.workgroup_id = Some("wg-pty-finding".to_string());
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, task.workgroup_id.clone());
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::codex::CodexAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            "[FINDING] PTY stream keeps group findings\n".to_string(),
        )
        .unwrap();

    let findings = store.list_findings("wg-pty-finding").unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].content, "PTY stream keeps group findings");
    assert_eq!(findings[0].source_task_id.as_deref(), Some(task.id.as_str()));
}

#[test]
fn session_id_is_saved_once_across_stream_events() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = pty_task("t-pty-session", TaskStatus::Running);
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            &crate::agent::codex::CodexAgent,
            &task.id,
            &store,
            log.as_file_mut(),
            concat!(
                "{\"type\":\"thread.started\",\"thread_id\":\"session-first\"}\n",
                "{\"type\":\"thread.started\",\"thread_id\":\"session-second\"}\n",
            )
            .to_string(),
        )
        .unwrap();

    let task = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(task.agent_session_id.as_deref(), Some("session-first"));
}

#[test]
fn finalize_streaming_persists_transcript() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task = Task {
        id: TaskId("t-pty-transcript".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
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
    };
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::new(true, None);
    state.full_output.push_str("complete transcript");

    finalize_streaming(
        &crate::agent::codex::CodexAgent,
        &task.id,
        &store,
        &portable_pty::ExitStatus::with_exit_code(0),
        &mut state,
    )
    .unwrap();

    let transcript = std::fs::read_to_string(paths::transcript_path(task.id.as_str())).unwrap();
    assert!(transcript.contains("complete transcript"));
    assert!(transcript.contains("=== AID TASK t-pty-transcript DONE (exit 0) ==="));
}

pub(super) fn pty_task(task_id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
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
