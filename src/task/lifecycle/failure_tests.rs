// Unknown-failure metadata regression tests through lifecycle settlement.
// Uses an isolated task store and provider-attributed diagnostic files.

use super::*;
use crate::support::paths;

#[test]
fn unknown_failures_record_one_provider_excerpt_without_changing_status() {
    let home = tempfile::tempdir().expect("home");
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("directories");
    let store = Store::open_memory().expect("store");
    let id = TaskId("t-unknown-provider".into());
    insert_task(&store, &id);
    std::fs::write(
        paths::stderr_path(id.as_str()),
        "Provider connection closed unexpectedly",
    )
    .expect("stderr");
    std::fs::write(paths::log_path(id.as_str()),
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"private assistant prose"}}"#)
        .expect("stdout");
    store
        .insert_agent_event(&TaskEvent {
            task_id: id.clone(),
            timestamp: chrono::Local::now(),
            event_kind: EventKind::Error,
            detail: "Original connection failure".into(),
            metadata: None,
        })
        .expect("original error");
    super::super::mark_failed(&store, &id).expect("failure");
    super::super::salvage_failed_task(&store, &id);
    assert_unknown_marker(&store, &id);
    assert_eq!(
        store
            .get_task(id.as_str())
            .expect("task")
            .expect("present")
            .status,
        crate::types::TaskStatus::Failed
    );
    assert!(!rate_limit::marker_path(&AgentKind::Codex, None).exists());
}

#[test]
fn recognised_failures_do_not_receive_unknown_markers() {
    let home = tempfile::tempdir().expect("home");
    let _guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().expect("directories");
    let store = Store::open_memory().expect("store");
    let id = TaskId("t-known-provider".into());
    insert_task(&store, &id);
    std::fs::write(
        paths::stderr_path(id.as_str()),
        "HTTP 429 Too Many Requests",
    )
    .expect("stderr");
    super::super::mark_failed(&store, &id).expect("failure");
    assert!(
        store
            .get_events(id.as_str())
            .expect("events")
            .iter()
            .all(|event| event
                .metadata
                .as_ref()
                .is_none_or(|data| data["unclassified_failure"] != true))
    );
}

fn insert_task(store: &Store, id: &TaskId) {
    use crate::types::{Task, TaskStatus, VerifyStatus};
    store
        .insert_task(&Task {
            id: id.clone(),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "failure classification".into(),
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
            created_at: chrono::Local::now(),
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        })
        .expect("insert task");
}

fn assert_unknown_marker(store: &Store, id: &TaskId) {
    let events = store.get_events(id.as_str()).expect("persisted events");
    let markers: Vec<_> = events
        .iter()
        .filter(|event| {
            event
                .metadata
                .as_ref()
                .is_some_and(|data| data["unclassified_failure"] == true)
        })
        .collect();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].event_kind, EventKind::Milestone);
    assert_eq!(
        markers[0].metadata.as_ref().expect("metadata")["provider_excerpt"],
        "Provider connection closed unexpectedly"
    );
    let errors: Vec<_> = events
        .iter()
        .filter(|event| event.event_kind == EventKind::Error)
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].detail, "Original connection failure");
}
