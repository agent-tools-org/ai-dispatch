// Session reuse and clearing across manual retry routes.
// Shares retry fixtures from retry_saved_args_tests.
use super::*;

/// `aid retry <id> --agent <other>` must drop both model AND session_id.
/// Before this fix, session_id was set based on whether the OLD agent supports
/// session resume — it was never cleared on agent change.
#[test]
fn retry_agent_switch_clears_session_id() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-session-switch");
    // Codex supports session resume; the session is recorded at task
    // completion and stored in agent_session_id.
    task.agent = AgentKind::Codex;
    task.agent_session_id = Some("session-abc123".to_string());
    task.requested_model = Some("gpt-5".to_string());
    store.insert_task(&task).unwrap();

    let args = retry_task_to_run_args(
        &store,
        &task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: Some("fix the bug".to_string()),
            feedback_file: None,
            agent: Some("gemini".to_string()), // switching to a different CLI
            model: None,
            idle_timeout_secs: None,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    )
    .unwrap();

    assert_eq!(args.agent_name, "gemini");
    assert!(
        args.session_id.is_none(),
        "session_id must be cleared when agent changes, got {:?}",
        args.session_id
    );
    // model must also be cleared (it is route-owned, not task-owned)
    assert!(args.model.is_none(), "model must be cleared when agent changes");
}

/// A same-agent retry must still resume its session.
#[test]
fn retry_same_agent_preserves_session_id() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-session-same");
    task.agent = AgentKind::Codex;
    task.agent_session_id = Some("session-xyz789".to_string());
    store.insert_task(&task).unwrap();

    let args = retry_args(&store, &task, None); // no --agent override → same agent

    assert_eq!(args.agent_name, "codex");
    assert_eq!(
        args.session_id.as_deref(),
        Some("session-xyz789"),
        "same-agent retry must preserve the session id"
    );
}
