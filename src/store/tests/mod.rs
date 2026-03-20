// Store tests module wiring and shared helpers.
// Exports: test submodules and make_task helper.
// Deps: Store, chrono.

use chrono::Local;

use super::*;
use crate::types::*;

fn make_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "test prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
            duration_ms: None,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at: Local::now(),
        completed_at: None,
        verify: None,
            verify_status: VerifyStatus::Skipped,
        read_only: false,
        budget: false,
    }
}

mod agent_tests;
mod db_tests;
mod event_tests;
mod task_tests;
mod workgroup_tests;
mod memory_tests;
