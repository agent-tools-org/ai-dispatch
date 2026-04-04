// ETA estimation helpers for running tasks in board views.
// Exports: estimate_eta for display-ready remaining-time labels.
// Deps: chrono, crate::store::Store, crate::types::Task.

use chrono::{DateTime, Local};

use crate::store::Store;
use crate::types::{Task, TaskStatus};

pub fn estimate_eta(task: &Task, store: &Store) -> Option<String> {
    estimate_eta_at(task, store, Local::now())
}

fn estimate_eta_at(task: &Task, store: &Store, now: DateTime<Local>) -> Option<String> {
    if task.status != TaskStatus::Running {
        return None;
    }
    let elapsed_ms = (now - task.created_at).num_milliseconds();
    let mut durations: Vec<i64> = store
        .recent_tasks_for_agent(task.agent, 50)
        .ok()?
        .into_iter()
        .filter_map(|entry| entry.duration_ms)
        .collect();
    if durations.len() < 3 {
        return None;
    }
    durations.sort_unstable();
    let median_ms = durations[durations.len() / 2];
    let remaining_ms = median_ms - elapsed_ms;
    if remaining_ms <= 0 {
        return Some("any moment".to_string());
    }
    Some(format_eta(remaining_ms))
}

fn format_eta(ms: i64) -> String {
    let secs = (ms / 1_000).max(0);
    if secs < 60 {
        format!("~{secs}s")
    } else if secs < 3_600 {
        format!("~{}m", secs / 60)
    } else {
        format!("~{}h{}m", secs / 3_600, (secs % 3_600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use crate::types::{AgentKind, TaskId, VerifyStatus};

    fn make_task(
        id: &str,
        agent: AgentKind,
        status: TaskStatus,
        created_at: DateTime<Local>,
        duration_ms: Option<i64>,
    ) -> Task {
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
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms,
            model: None,
            cost_usd: None,
            exit_code: None,
            created_at,
            completed_at: None,
            verify: None,
            verify_status: VerifyStatus::Skipped,
            pending_reason: None,
            read_only: false,
            budget: false,
        }
    }

    #[test]
    fn estimate_eta_returns_none_without_history() {
        let store = Store::open_memory().unwrap();
        let now = Local::now();
        store
            .insert_task(&make_task(
                "t-done-1",
                AgentKind::Codex,
                TaskStatus::Done,
                now - Duration::minutes(10),
                Some(120_000),
            ))
            .unwrap();
        store
            .insert_task(&make_task(
                "t-done-2",
                AgentKind::Codex,
                TaskStatus::Done,
                now - Duration::minutes(20),
                Some(180_000),
            ))
            .unwrap();

        let running = make_task(
            "t-run",
            AgentKind::Codex,
            TaskStatus::Running,
            now - Duration::seconds(30),
            None,
        );

        assert_eq!(estimate_eta_at(&running, &store, now), None);
    }

    #[test]
    fn estimate_eta_returns_remaining_time() {
        let store = Store::open_memory().unwrap();
        let now = Local::now();
        for (id, minutes_ago, duration_ms) in [
            ("t-done-1", 10, 120_000),
            ("t-done-2", 20, 180_000),
            ("t-done-3", 30, 240_000),
        ] {
            store
                .insert_task(&make_task(
                    id,
                    AgentKind::Codex,
                    TaskStatus::Done,
                    now - Duration::minutes(minutes_ago),
                    Some(duration_ms),
                ))
                .unwrap();
        }

        let running = make_task(
            "t-run",
            AgentKind::Codex,
            TaskStatus::Running,
            now - Duration::seconds(60),
            None,
        );

        assert_eq!(estimate_eta_at(&running, &store, now), Some("~2m".to_string()));
    }

    #[test]
    fn format_eta_formats_correctly() {
        assert_eq!(format_eta(59_000), "~59s");
        assert_eq!(format_eta(60_000), "~1m");
        assert_eq!(format_eta(3_720_000), "~1h2m");
    }
}
