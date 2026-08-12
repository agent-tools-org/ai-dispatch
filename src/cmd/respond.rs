// Handler for `aid respond` input forwarding.
// Writes one response payload for a background task under ~/.aid/jobs.

use anyhow::{Context, Result};

use crate::cmd::reply::{InputCommand, ensure_interactive_input};
use crate::input_signal;
use crate::store::Store;

pub fn run(store: &Store, task_id: &str, input: Option<&str>, file: Option<&str>) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task {task_id} not found"))?;
    ensure_interactive_input(&task, InputCommand::Respond)?;
    let text = if let Some(path) = file {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read response file: {path}"))?
    } else if let Some(text) = input {
        text.to_string()
    } else {
        use std::io::Read;

        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read from stdin")?;
        buf
    };
    input_signal::write_response(task_id, &text)?;
    println!("Queued input for {task_id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::input_signal;
    use crate::paths::AidHomeGuard;
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    fn make_task(id: &str, agent: AgentKind) -> Task {
        Task {
            id: TaskId(id.to_string()), agent, custom_agent_name: None,
            prompt: "test".to_string(), resolved_prompt: None, category: None,
            status: TaskStatus::Running, parent_task_id: None, workgroup_id: None,
            caller_kind: None, caller_session_id: None, agent_session_id: None,
            repo_path: None, project_id: None, worktree_path: None, effective_dir: None, worktree_branch: None,
            final_head_sha: None, final_branch: None, start_sha: None, log_path: None,
            output_path: None, tokens: None, prompt_tokens: None, duration_ms: None,
            requested_model: None, observed_model: None, attribution_source: None,
            cost_usd: None, exit_code: None, created_at: Local::now(), completed_at: None,
            verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None,
            read_only: false, budget: false, audit_verdict: None, audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn respond_rejects_one_shot_agents_without_writing_signal() {
        let temp_home = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp_home.path());
        let store = Store::open_memory().unwrap();
        for (task_id, agent) in [("t-respond-agy", AgentKind::Antigravity), ("t-respond-grok", AgentKind::Grok)] {
            store.insert_task(&make_task(task_id, agent)).unwrap();
            let err = run(&store, task_id, Some("yes"), None).unwrap_err();
            assert!(err.to_string().contains("no response signal was written"));
            assert!(input_signal::take_response(task_id).unwrap().is_none());
        }
    }

    #[test]
    fn respond_keeps_codex_delivery_path() {
        let temp_home = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp_home.path());
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-respond-codex", AgentKind::Codex)).unwrap();
        run(&store, "t-respond-codex", Some("yes"), None).unwrap();
        assert_eq!(
            input_signal::take_response("t-respond-codex").unwrap().as_deref(),
            Some("yes")
        );
    }

    #[test]
    fn respond_explains_how_to_recover_deleted_custom_agent() {
        let temp_home = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp_home.path());
        let store = Store::open_memory().unwrap();
        let mut task = make_task("t-respond-missing-custom", AgentKind::Custom);
        task.custom_agent_name = Some("gone".to_string());
        store.insert_task(&task).unwrap();

        let err = run(&store, task.id.as_str(), Some("yes"), None).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("unavailable custom agent 'gone'"));
        assert!(message.contains("restore ~/.aid/agents/gone.toml"));
        assert!(message.contains("stop the task and retry"));
    }
}
