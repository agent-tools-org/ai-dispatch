// Dispatch evidence coverage: one event per task, optional balances, explicit exhausted routes.
// Uses an isolated aidbar cache and AID_HOME with an in-memory store.

use super::*;
use crate::routing::quota::live::CacheDirGuard;
use crate::support::paths::AidHomeGuard;
use crate::types::{Task, TaskStatus, VerifyStatus};

fn task() -> Task {
    Task {
        id: TaskId("t-billing".into()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".into(),
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
    }
}

#[test]
fn dispatch_records_overage_once_and_does_not_block_exhausted() {
    let temp = tempfile::tempdir().expect("temp");
    let _home = AidHomeGuard::set(temp.path());
    let _cache = CacheDirGuard::set(temp.path());
    let store = Store::open_memory().expect("store");
    let task = task();
    store.insert_task(&task).expect("task");
    for (state, expected) in [("exhausted", 0), ("in_plan", 0), ("overage", 1), ("overage", 1)] {
        std::fs::write(
            temp.path().join("codex.json"),
            serde_json::json!({
                "ok":true,"snapshot":{"provider":"codex","windows":[],
                    "fetched_at":"2099-01-01T00:00:00Z","billing":{"state":state,
                        "credits":{"balance":12.25,"unlimited":false}}}
            })
            .to_string(),
        )
        .expect("cache");
        record(&store, &task.id, task.agent).expect("explicit dispatch permitted");
        let events = store.get_events(task.id.as_str()).expect("events");
        assert_eq!(events.len(), expected);
        if let Some(event) = events.first() {
            assert_eq!(event.detail, "billing: overage (credits balance 12.25)");
            assert_eq!(
                event.metadata.as_ref().expect("metadata")["billing_state"],
                "overage"
            );
        }
    }
}
