// progress tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

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
    let nudge = crate::runtime::pty::watch_idle::default_nudge_message();
    crate::runtime::pty::watch_idle::register_inbound_echo(
        &mut state.inbound_echo_suppress,
        nudge.clone(),
    );
    let mut log = tempfile::NamedTempFile::new().unwrap();

    // Stream goes silent except for aid's own nudge echoes (the live failure shape).
    state
        .handle_chunk(
            crate::agent::get_agent(AgentKind::OpenCode).as_ref(),
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
    let nudge = crate::runtime::pty::watch_idle::default_nudge_message();
    crate::runtime::pty::watch_idle::register_inbound_echo(
        &mut state.inbound_echo_suppress,
        nudge.clone(),
    );
    let mut log = tempfile::NamedTempFile::new().unwrap();

    state
        .handle_chunk(
            crate::agent::get_agent(AgentKind::OpenCode).as_ref(),
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
            crate::agent::get_agent(AgentKind::OpenCode).as_ref(),
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
    assert_eq!(
        findings[0].source_task_id.as_deref(),
        Some(task.id.as_str())
    );
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
