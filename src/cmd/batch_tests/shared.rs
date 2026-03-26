// Shared test helpers for batch submodule tests.
// Exports: make_task, seed_task, make_stored_task
// Deps: crate::{batch,store,types}
use crate::batch;
use crate::store::{Store, TaskCompletionUpdate};
use crate::types::Task;
use crate::types::{AgentKind, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

pub(super) fn make_task(name: &str, conditional: bool, on_success: Option<&str>) -> batch::BatchTask {
    batch::BatchTask {
        id: None,
        name: Some(name.to_string()),
        agent: "codex".to_string(),
        team: None,
        prompt: "prompt".to_string(),
        dir: None,
        output: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        best_of: None,
        max_duration_mins: None,
        idle_timeout: None,
        verify: None,
        judge: None,
        context: None,
        checklist: None,
        skills: None,
        hooks: None,
        depends_on: None,
        parent: None,
        context_from: None,
        fallback: None,
        scope: None,
        read_only: false,
        budget: false,
        env: None,
        env_forward: None,
        on_success: on_success.map(str::to_string),
        on_fail: None,
        conditional,
    }
}

pub(super) fn seed_task(store: &Store, task_id: &str, status: TaskStatus, cost_usd: Option<f64>) {
    store.insert_waiting_task(task_id, "codex", "prompt", None).unwrap();
    store
        .update_task_completion(TaskCompletionUpdate {
            id: task_id,
            status,
            tokens: None,
            duration_ms: 1_000,
            model: Some("gpt-5"),
            cost_usd,
            exit_code: None,
        })
        .unwrap();
}

pub(super) fn make_stored_task(id: &str, agent: AgentKind, status: TaskStatus) -> Task {
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
        pending_reason: None,
        read_only: false,
        budget: false,
    }
}
