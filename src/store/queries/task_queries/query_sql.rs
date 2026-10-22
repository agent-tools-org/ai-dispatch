// SQL and parameter selection for task list filters.
// Depends on task filter/status types and the local clock.

use super::*;

pub(super) fn list_tasks_query(filter: TaskFilter) -> (&'static str, Vec<String>) {
    match filter {
        TaskFilter::All => (ALL_SQL, vec![]),
        TaskFilter::Active => (
            ACTIVE_SQL,
            ACTIVE_TASK_STATUSES
                .iter()
                .map(|status| status.as_str().to_string())
                .collect(),
        ),
        TaskFilter::Running => (
            RUNNING_SQL,
            vec![
                "running".to_string(),
                "awaiting_input".to_string(),
                "stalled".to_string(),
            ],
        ),
        TaskFilter::Today => today_tasks_query(),
    }
}

fn today_tasks_query() -> (&'static str, Vec<String>) {
    let today = Local::now().format("%Y-%m-%d").to_string();
    (TODAY_SQL, vec![today])
}

const ALL_SQL: &str = "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, \
            worktree_branch,
                 start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
                 created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
                 exit_code, category, pending_reason, audit_verdict, audit_report_path, \
            delivery_assessment, final_head_sha, final_branch, observed_model, attribution_source, \
            project_id, effective_dir
                 FROM tasks ORDER BY created_at DESC";

const ACTIVE_SQL: &str = "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, \
            workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, \
            worktree_branch,
                 start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
                 created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
                 exit_code, category, pending_reason, audit_verdict, audit_report_path, \
            delivery_assessment, final_head_sha, final_branch, observed_model, attribution_source, \
            project_id, effective_dir
                 FROM tasks WHERE status IN (?1, ?2, ?3, ?4, ?5) ORDER BY created_at DESC";

const RUNNING_SQL: &str = "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, \
            workgroup_id,
                 caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, \
            worktree_branch,
                 start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
                 created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
                 exit_code, category, pending_reason, audit_verdict, audit_report_path, \
            delivery_assessment, final_head_sha, final_branch, observed_model, attribution_source, \
            project_id, effective_dir
                 FROM tasks WHERE status IN (?1, ?2, ?3) ORDER BY created_at DESC";

const TODAY_SQL: &str = "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
                     caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, \
            worktree_branch,
                     start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, \
            cost_usd,
                     created_at, completed_at, verify, read_only, budget, custom_agent_name, \
            verify_status,
                     exit_code, category, pending_reason, audit_verdict, audit_report_path, \
            delivery_assessment, final_head_sha, final_branch, observed_model, attribution_source, \
            project_id, effective_dir
                     FROM tasks WHERE created_at >= ?1 ORDER BY created_at DESC";
