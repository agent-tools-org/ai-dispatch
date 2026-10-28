// Test-only task baseline with empty optional metadata.
// Requires identity and status; scenario-specific metadata stays at each call site.
// Deps: Task and its domain enums; never compiled into the production binary.

use super::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

impl Task {
    pub(crate) fn test_fixture(id: TaskId, status: TaskStatus) -> Self {
        Self {
            id,
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "prompt".into(),
            resolved_prompt: None,
            category: None,
            status,
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
}
