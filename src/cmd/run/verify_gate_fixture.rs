// Provisional task fixture for verification-gate tests.
// Deps: task record types and the local clock.

use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

pub(super) fn task(id: &str, status: TaskStatus, dir: Option<&str>, verify: Option<&str>) -> Task {
    Task {
        id: TaskId(id.to_string()),
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
        worktree_path: dir.map(str::to_string), effective_dir: None,
        worktree_branch: Some("fix/verify-gate".to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: Some(10),
        requested_model: None, observed_model: None, attribution_source: None,
        cost_usd: None,
        exit_code: Some(0),
        created_at: Local::now(),
        completed_at: None,
        verify: verify.map(str::to_string),
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    }
}

