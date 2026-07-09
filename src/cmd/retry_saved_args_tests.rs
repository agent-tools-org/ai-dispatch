// Tests for `aid retry` dispatch-args rehydration.
// Covers saved RunArgs reuse, secret env omission, and CLI override precedence.
// Deps: retry builder, run::RunArgs, Store, task domain types.

use super::{retry_task_to_run_args, RetryArgs};
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::collections::HashMap;

fn failed_task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "original prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Failed, parent_task_id: None, workgroup_id: Some("wg-old".to_string()),
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None,
        worktree_path: None, worktree_branch: None, final_head_sha: None, final_branch: None, start_sha: None, log_path: None,
        output_path: None, tokens: None, prompt_tokens: None, duration_ms: None, model: None,
        cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
        verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None,
        read_only: false, budget: false, audit_verdict: None, audit_report_path: None,
        delivery_assessment: None,
    }
}

#[test]
fn retry_rehydrates_saved_context_scope_team_and_agent_override_wins() {
    let store = Store::open_memory().unwrap();
    let task = failed_task("t-retry");
    store.insert_task(&task).unwrap();
    let mut env = HashMap::new();
    env.insert("TOKEN".to_string(), "secret".to_string());
    let saved = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "original prompt".to_string(),
        context: vec!["src/lib.rs".to_string()],
        scope: vec!["src/**".to_string()],
        team: Some("dev".to_string()),
        env: Some(env),
        ..Default::default()
    };
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap()).unwrap();

    let args = retry_task_to_run_args(
        &store,
        &task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: "fix it".to_string(),
            agent: Some("gemini".to_string()),
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    ).unwrap();

    assert_eq!(args.agent_name, "gemini");
    assert_eq!(args.context, vec!["src/lib.rs".to_string()]);
    assert_eq!(args.scope, vec!["src/**".to_string()]);
    assert_eq!(args.team, Some("dev".to_string()));
    assert_eq!(args.parent_task_id, Some("t-retry".to_string()));
    assert!(args.env.is_none());
}
