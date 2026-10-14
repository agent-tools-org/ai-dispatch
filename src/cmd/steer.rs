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
    println!("Queued steer for {task_id}: {}", message.chars().take(80).collect::<String>());
    Ok(())
}

pub(crate) fn wrap_up_before_timeout(
    store: &Store,
    task_id: &str,
    elapsed_secs: u64,
    cap_secs: u64,
) -> Result<bool> {
    if cap_secs < 600 || elapsed_secs < cap_secs - cap_secs / 10 || elapsed_secs > cap_secs {
        return Ok(false);
    }
    let Some(task) = store.get_task(task_id)? else {
        return Ok(false);
    };
    if task.read_only || reply::ensure_interactive_input(&task, InputCommand::Steer).is_err()
        || crate::paths::steer_signal_path(task_id).exists()
    {
        return Ok(false);
    }
    queue_wrap_up(store, task_id)
}

fn queue_wrap_up(store: &Store, task_id: &str) -> Result<bool> {
    let mut conn = store.db();
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if crate::paths::steer_signal_path(task_id).exists() {
        return Ok(false);
    }
    let message = "Hard timeout is approaching: please commit work in progress if changes are permitted, \
                   and write the report now.";
    let rows = tx.execute(
        "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
         SELECT id, ?2, 'milestone', ?3, ?4 FROM tasks WHERE id = ?1
         AND status IN ('running', 'awaiting_input', 'stalled') AND read_only = 0
         AND NOT EXISTS (SELECT 1 FROM events WHERE task_id = ?1 AND
             CASE WHEN json_valid(metadata)
             THEN json_extract(metadata, '$.hard_cap_wrap_up') END = 1)",
        rusqlite::params![
            task_id,
            chrono::Local::now().to_rfc3339(),
            "Hard-cap wrap-up: queued request to commit work in progress and write the report",
            r#"{"input_delivery":"queued","hard_cap_wrap_up":true}"#
        ],
    )?;
    if rows == 0 {
        return Ok(false);
    }
    tx.execute(
        "INSERT INTO task_messages (task_id, direction, content, source, created_at)
         VALUES (?1, 'in', ?2, 'steer', ?3)",
        rusqlite::params![task_id, message, chrono::Local::now().to_rfc3339()],
    )?;
    tx.commit()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{run, wrap_up_before_timeout};
    use crate::input_signal;
    use crate::paths::AidHomeGuard;
    use crate::store::Store;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    fn make_task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Gemini,
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
            repo_path: None, project_id: None,
            worktree_path: None, effective_dir: None,
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
    fn concurrent_wrap_up_pollers_queue_only_one_steer() {
        let home = tempfile::tempdir().expect("home");
        let store = Store::open(&home.path().join("aid.db")).expect("store");
        store
            .insert_task(&make_task("t-concurrent", TaskStatus::Running))
            .expect("task");
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                let barrier = &barrier;
                let path = home.path();
                scope.spawn(move || {
                    let _home = AidHomeGuard::set(path);
                    let store = Store::open(&path.join("aid.db")).expect("store");
                    barrier.wait();
                    wrap_up_before_timeout(&store, "t-concurrent", 540, 600).expect("wrap-up");
                });
            }
        });
        assert_eq!(
            store
                .list_messages_for_task("t-concurrent")
                .expect("messages")
                .len(),
            1
        );
        assert_eq!(store.get_events("t-concurrent").expect("events").len(), 1);
    }

    #[test]
    fn wrap_up_preserves_pending_operator_steer_and_skips_expired_cap() {
        let home = tempfile::tempdir().expect("home");
        let _home = AidHomeGuard::set(home.path());
        let store = Store::open_memory().expect("store");
        store
            .insert_task(&make_task("t-pending", TaskStatus::Running))
            .expect("task");
        input_signal::write_steer("t-pending", "operator direction").expect("steer");
        assert!(!wrap_up_before_timeout(&store, "t-pending", 540, 600).expect("wrap-up"));
        assert_eq!(
            input_signal::take_steer("t-pending").expect("steer").as_deref(),
            Some("operator direction")
        );
        assert!(!wrap_up_before_timeout(&store, "t-pending", 601, 600).expect("expired cap"));
        assert!(
            store
                .list_messages_for_task("t-pending")
                .expect("messages")
                .is_empty()
        );
        assert!(store.get_events("t-pending").expect("events").is_empty());
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
            let message = err.to_string();
            assert!(message.contains("no steer message was queued"), "{message}");
            assert!(message.contains(&format!("`aid stop {task_id}` then `aid retry {task_id} -f")), "{message}");
            assert!(store.list_messages_for_task(task_id).unwrap().is_empty());
            assert!(!crate::paths::steer_signal_path(task_id).exists());
        }
    }

    #[test]
    fn steer_refuses_codex_without_queuing() {
        let temp_home = tempfile::tempdir().unwrap();
        let _aid_home = AidHomeGuard::set(temp_home.path());
        let store = Store::open_memory().unwrap();
        let mut task = make_task("t-steer-codex", TaskStatus::Running);
        task.agent = AgentKind::Codex;
        store.insert_task(&task).unwrap();
        let error = run(&store, task.id.as_str(), "pivot").unwrap_err();
        assert!(error.to_string().contains("aid retry t-steer-codex -f"));
        assert!(store.list_messages_for_task(task.id.as_str()).unwrap().is_empty());
        assert!(!crate::paths::steer_signal_path(task.id.as_str()).exists());
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
