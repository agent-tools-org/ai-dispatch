// transcript tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

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
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
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
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    };
    store.insert_task(&task).unwrap();
    let mut state = MonitorState::with_policy(
        true,
        None,
        crate::config::timeout_policy::TimeoutPolicy::default(),
    );
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
