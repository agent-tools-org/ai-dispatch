// Verification infrastructure regression tests at the task outcome boundary.
// Exports: no production API; tests exact target denial and genuine Cargo failure.
// Deps: run::maybe_verify, Store, task outcome types, tempfile.

use super::maybe_verify;
use crate::types::outcome::UnverifiedReason;
use crate::{
    store::Store,
    types::{AgentKind, Task, TaskId, TaskOutcome, TaskStatus, VerifyStatus},
};
use chrono::Local;

fn task(id: &str, dir: &str, verify: &str) -> Task {
    Task {
        id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Done, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None,
        repo_path: None, project_id: None, worktree_path: Some(dir.to_string()), effective_dir: None,
        worktree_branch: Some("fix/verify-gate".to_string()), final_head_sha: None,
        final_branch: None, start_sha: None, log_path: None, output_path: None,
        tokens: None, prompt_tokens: None, duration_ms: Some(10), requested_model: None,
        observed_model: None, attribution_source: None, cost_usd: None, exit_code: Some(0),
        created_at: Local::now(), completed_at: Some(Local::now()), verify: Some(verify.to_string()),
        verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false,
        budget: false, audit_verdict: None, audit_report_path: None, delivery_assessment: None,
    }
}

#[cfg(unix)]
#[test]
fn cargo_target_permission_diagnostic_is_unverified_infrastructure() {
    let _permit = crate::test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    let dir_str = dir.path().to_string_lossy().to_string();
    let target = crate::agent::target_dir_for_worktree(Some("fix/verify-gate")).unwrap();
    let script = dir.path().join("target-denied.sh");
    std::fs::write(&script, format!(
        "#!/bin/sh\necho 'error: error writing dependencies to `{target}/debug/deps/foo.d`: Operation not permitted (os error 1)' >&2\nexit 1\n"
    )).unwrap();
    let command = format!("sh {}", script.display());
    let store = Store::open_memory().unwrap();
    let task_id = TaskId("t-target-denied".to_string());
    store.insert_task(&task(task_id.as_str(), &dir_str, &command)).unwrap();

    maybe_verify(&store, &task_id, Some(&command), Some(&dir_str), None);

    let loaded = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.verify_status, VerifyStatus::InfrastructureFailure);
    assert_eq!(loaded.status, TaskStatus::Done);
    assert_eq!(loaded.outcome(), TaskOutcome::Unverified(UnverifiedReason::Infrastructure));
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.detail.contains("Operation not permitted") && event.detail.contains(&target)
    }));
}
