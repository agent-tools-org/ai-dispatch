// Tests for persisted result-file output fallback in show helpers.
// Ensures `result.md` is treated as the primary rendered output when present.

use crate::cmd::show::read_task_output;
use crate::paths::AidHomeGuard;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

fn task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: Some("research".to_string()),
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
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
        read_only: true,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn read_task_output_uses_persisted_result_file() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = AidHomeGuard::set(temp.path());
    let result_path = crate::paths::task_dir("t-result-default").join("result.md");
    std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
    std::fs::write(&result_path, "## Findings\nNo findings.\n").unwrap();

    let output = read_task_output(&task("t-result-default")).unwrap();

    assert_eq!(output, "## Findings\nNo findings.\n");
}

#[test]
fn read_task_output_unwraps_persisted_grok_envelope() {
    let temp = tempfile::tempdir().unwrap();
    let output_path = temp.path().join("grok-output.json");
    let _aid_home = AidHomeGuard::set(temp.path());
    let task = Task {
        agent: AgentKind::Grok,
        output_path: Some(output_path.display().to_string()),
        ..task("t-grok-envelope")
    };
    let envelope = serde_json::json!({
        "text": "# Findings\n\nThe report is rendered markdown."
    });
    std::fs::write(&output_path, serde_json::to_string(&envelope).unwrap()).unwrap();

    let output = read_task_output(&task).unwrap();

    assert_eq!(output, "# Findings\n\nThe report is rendered markdown.");
}
