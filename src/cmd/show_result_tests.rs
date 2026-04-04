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
        start_sha: None,
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
        read_only: true,
        budget: false,
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
