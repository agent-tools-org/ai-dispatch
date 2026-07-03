// Tests for cascade retry argument inheritance.
// Covers worktree/dir reuse when a fallback agent takes over a failed task.
// Deps: run_lifecycle helper, RunArgs, task domain types.

use super::{run_lifecycle::inherit_cascade_target, RunArgs};
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;

fn failed_task(path: &str) -> Task {
    Task {
        id: TaskId("t-cascade".to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None,
        worktree_path: Some(path.to_string()), worktree_branch: Some("feat/cascade".to_string()),
        start_sha: None, log_path: None, output_path: None, tokens: None, prompt_tokens: None,
        duration_ms: None, model: None, cost_usd: None, exit_code: None,
        created_at: Local::now(), completed_at: None, verify: None,
        verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false,
        budget: false, audit_verdict: None, audit_report_path: None, delivery_assessment: None,
    }
}

#[test]
fn cascade_inherits_existing_worktree_dir() {
    let temp = tempfile::tempdir().unwrap();
    let task = failed_task(&temp.path().display().to_string());
    let mut args = RunArgs {
        agent_name: "gemini".to_string(),
        worktree: Some("feat/cascade".to_string()),
        ..Default::default()
    };

    inherit_cascade_target(&mut args, &task);

    assert_eq!(args.dir, task.worktree_path);
    assert_eq!(args.worktree, None);
}
