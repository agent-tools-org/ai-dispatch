// State computation queries — project-scoped aggregations from task history.
// Exports: Store helpers for recent project tasks and project-level state metrics.
// Deps: super::super::{schema::row_to_task, Store}, rusqlite, crate::types::Task.

use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use super::super::schema::row_to_task;
use super::super::Store;
use crate::types::Task;

impl Store {
    pub fn recent_tasks_for_project(&self, repo_path: &str, limit: usize) -> Result<Vec<Task>> {
        if limit == 0 {
            return Ok(vec![]);
        }

        let conn = self.db();
        let limit = i64::try_from(limit)?;
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
             created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
             exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment
             FROM tasks
             WHERE repo_path = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![repo_path, limit], row_to_task)?;
        rows.map(|row| row?).collect()
    }

    pub fn project_agent_success_rates(&self, repo_path: &str) -> Result<Vec<(String, f64, usize)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent,
                    COUNT(*) as total,
                    SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) as success
             FROM tasks
             WHERE repo_path = ?1 AND status IN ('done', 'failed')
             GROUP BY agent
             HAVING total >= 3",
        )?;
        let rows = stmt.query_map(params![repo_path], |row| {
            let agent: String = row.get(0)?;
            let total: i64 = row.get(1)?;
            let success: i64 = row.get(2)?;
            let rate = success as f64 / total as f64;
            Ok((agent, rate, total as usize))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn last_verify_event(&self, repo_path: &str) -> Result<Option<(String, String)>> {
        let conn = self.db();
        let event = conn
            .query_row(
                "SELECT e.detail, e.timestamp
                 FROM events e
                 JOIN tasks t ON e.task_id = t.id
                 WHERE t.repo_path = ?1 AND e.event_type = 'verify'
                 ORDER BY e.timestamp DESC
                 LIMIT 1",
                params![repo_path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(event)
    }

    pub fn project_avg_cost(&self, repo_path: &str) -> Result<Option<f64>> {
        let conn = self.db();
        let avg_cost = conn.query_row(
            "SELECT AVG(cost_usd)
             FROM tasks
             WHERE repo_path = ?1
               AND status = 'done'
               AND cost_usd IS NOT NULL
               AND created_at > datetime('now', '-30 days')",
            params![repo_path],
            |row| row.get(0),
        )?;
        Ok(avg_cost)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, Local};
    use rusqlite::params;

    use super::*;
    use crate::types::{AgentKind, TaskStatus};

    fn insert_task(
        store: &Store,
        id: &str,
        repo_path: &str,
        agent: &str,
        status: TaskStatus,
        created_at: &str,
        cost_usd: Option<f64>,
    ) {
        let conn = store.db();
        conn.execute(
            "INSERT INTO tasks (id, agent, prompt, status, repo_path, cost_usd, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, agent, format!("prompt-{id}"), status.as_str(), repo_path, cost_usd, created_at],
        )
        .unwrap();
    }

    fn insert_verify_event(store: &Store, task_id: &str, timestamp: &str, detail: &str) {
        let conn = store.db();
        conn.execute(
            "INSERT INTO events (task_id, timestamp, event_type, detail, metadata)
             VALUES (?1, ?2, 'verify', ?3, NULL)",
            params![task_id, timestamp, detail],
        )
        .unwrap();
    }

    fn rfc3339_now_minus(days: i64) -> String {
        (Local::now() - Duration::days(days)).to_rfc3339()
    }

    fn parse_local(timestamp: &str) -> DateTime<Local> {
        DateTime::parse_from_rfc3339(timestamp)
            .unwrap()
            .with_timezone(&Local)
    }

    #[test]
    fn recent_tasks_for_project_and_last_verify_event_are_project_scoped() {
        let store = Store::open_memory().unwrap();
        let older = "2026-03-18T10:00:00Z";
        let newer = "2026-03-19T10:00:00Z";

        insert_task(&store, "t-older", "/repo/a", AgentKind::Codex.as_str(), TaskStatus::Done, older, None);
        insert_task(&store, "t-newer", "/repo/a", AgentKind::Gemini.as_str(), TaskStatus::Failed, newer, None);
        insert_task(&store, "t-other", "/repo/b", AgentKind::Cursor.as_str(), TaskStatus::Done, newer, None);
        insert_verify_event(&store, "t-older", "2026-03-18T11:00:00Z", "older verify");
        insert_verify_event(&store, "t-newer", "2026-03-19T11:00:00Z", "latest verify");
        insert_verify_event(&store, "t-other", "2026-03-20T11:00:00Z", "other repo verify");

        let tasks = store.recent_tasks_for_project("/repo/a", 2).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id.as_str(), "t-newer");
        assert_eq!(tasks[0].repo_path.as_deref(), Some("/repo/a"));
        assert_eq!(tasks[1].id.as_str(), "t-older");
        assert_eq!(tasks[0].created_at, parse_local(newer));

        let event = store.last_verify_event("/repo/a").unwrap();
        assert_eq!(event, Some(("latest verify".to_string(), "2026-03-19T11:00:00Z".to_string())));
    }

    #[test]
    fn project_agent_success_rates_and_avg_cost_filter_to_project_history() {
        let store = Store::open_memory().unwrap();
        let recent_a = rfc3339_now_minus(5);
        let recent_b = rfc3339_now_minus(3);
        let recent_c = rfc3339_now_minus(2);
        let old_done = rfc3339_now_minus(45);

        insert_task(&store, "t-done-1", "/repo/a", "codex", TaskStatus::Done, &recent_a, Some(3.0));
        insert_task(&store, "t-done-2", "/repo/a", "codex", TaskStatus::Done, &recent_b, Some(9.0));
        insert_task(&store, "t-failed", "/repo/a", "codex", TaskStatus::Failed, &recent_c, Some(4.0));
        insert_task(&store, "t-old", "/repo/a", "codex", TaskStatus::Done, &old_done, Some(100.0));
        insert_task(&store, "t-other-1", "/repo/b", "codex", TaskStatus::Done, &recent_a, Some(50.0));
        insert_task(&store, "t-other-2", "/repo/a", "gemini", TaskStatus::Done, &recent_a, Some(7.0));
        insert_task(&store, "t-other-3", "/repo/a", "gemini", TaskStatus::Failed, &recent_b, Some(8.0));

        let rates = store.project_agent_success_rates("/repo/a").unwrap();
        assert_eq!(rates.len(), 1);
        assert_eq!(rates[0].0, "codex");
        assert_eq!(rates[0].2, 4);
        assert!((rates[0].1 - 0.75).abs() < f64::EPSILON);

        let avg_cost = store.project_avg_cost("/repo/a").unwrap();
        assert!(avg_cost.is_some());
        assert!((avg_cost.unwrap() - (19.0 / 3.0)).abs() < f64::EPSILON);
    }
}
