// Handler for `aid steer` — inject guidance into running PTY tasks.
// Delegates to persisted reply delivery with steer source tracking.

use anyhow::Result;

use crate::cmd::reply::{self, InputCommand};
use crate::store::Store;
use crate::types::MessageSource;

pub fn run(store: &Store, task_id: &str, message: &str) -> Result<()> {
    reply::run_with_source(
        store,
        task_id,
        Some(message),
        None,
        true,
        30,
        MessageSource::Steer,
        InputCommand::Steer,
    )?;
    println!("Steered {task_id}: {}", message.chars().take(80).collect::<String>());
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

    fn make_task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "test".to_string(),
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
            read_only: false,
            budget: false,
            audit_verdict: None,
            audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn steer_non_running_task_errors() {
        let store = Store::open_memory().unwrap();
        store.insert_task(&make_task("t-steer", TaskStatus::Done)).unwrap();
        let err = run(&store, "t-steer", "pivot").unwrap_err();
        // Steer now delegates to `aid reply`, so the error comes from the reply
        // path. Accept either phrasing so future wording tweaks don't break it.
        let msg = err.to_string();
        assert!(
            msg.contains("can only steer running tasks")
                || msg.contains("can only reply to running tasks"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn steer_rejects_one_shot_agents_without_queuing() {
        let temp_home = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp_home.path());
        let store = Store::open_memory().unwrap();

        for (task_id, agent) in [
            ("t-steer-agy", AgentKind::Antigravity),
            ("t-steer-grok", AgentKind::Grok),
        ] {
            let mut task = make_task(task_id, TaskStatus::Running);
            task.agent = agent;
            store.insert_task(&task).unwrap();

            let err = run(&store, task_id, "pivot").unwrap_err();
            assert!(err.to_string().contains("no steer message was queued"));
            assert!(store.list_messages_for_task(task_id).unwrap().is_empty());
            assert!(!crate::paths::steer_signal_path(task_id).exists());
        }
    }

    #[test]
    fn steer_keeps_codex_delivery_path() {
        let temp_home = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp_home.path());
        let store = Store::open_memory().unwrap();
        let task = make_task("t-steer-codex", TaskStatus::Running);
        store.insert_task(&task).unwrap();

        run(&store, task.id.as_str(), "pivot").unwrap();

        let messages = store.list_messages_for_task(task.id.as_str()).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].source.as_str(), "steer");
        assert_eq!(
            input_signal::take_steer(task.id.as_str()).unwrap().as_deref(),
            Some("pivot")
        );
    }

    #[test]
    fn steer_explains_how_to_recover_deleted_custom_agent() {
        let temp_home = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp_home.path());
        let store = Store::open_memory().unwrap();
        let mut task = make_task("t-steer-missing-custom", TaskStatus::Running);
        task.agent = AgentKind::Custom;
        task.custom_agent_name = Some("gone".to_string());
        store.insert_task(&task).unwrap();

        let err = run(&store, task.id.as_str(), "pivot").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("unavailable custom agent 'gone'"));
        assert!(message.contains("restore ~/.aid/agents/gone.toml"));
        assert!(message.contains("stop the task and retry"));
    }
}
