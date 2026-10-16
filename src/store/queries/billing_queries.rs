// Indexed dispatch billing evidence for usage reporting.
// Returns task IDs only; task creation dates match existing usage period semantics.

use super::super::Store;
use chrono::{DateTime, Local};
use rusqlite::params;
use std::collections::HashSet;

const OVERAGE_TASK_IDS_SQL: &str = "SELECT DISTINCT e.task_id
    FROM events e INDEXED BY idx_events_overage
    CROSS JOIN tasks t ON t.id = e.task_id
    WHERE e.event_type = 'setup'
      AND json_extract(CASE WHEN json_valid(e.metadata) THEN e.metadata END,
                       '$.billing_state') = 'overage'
      AND (?1 IS NULL OR julianday(t.created_at) >= julianday(?1))
      AND (?2 IS NULL OR julianday(t.created_at) <= julianday(?2))";

impl Store {
    pub(crate) fn api_route_month_tasks(
        &self,
        route: &str,
        start: DateTime<chrono::Utc>,
        end: DateTime<chrono::Utc>,
    ) -> anyhow::Result<Vec<crate::types::Task>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT id, agent, prompt, resolved_prompt, status, parent_task_id, workgroup_id,
             caller_kind, caller_session_id, agent_session_id, repo_path, worktree_path, worktree_branch,
             start_sha, log_path, output_path, tokens, prompt_tokens, duration_ms, model, cost_usd,
             created_at, completed_at, verify, read_only, budget, custom_agent_name, verify_status,
             exit_code, category, pending_reason, audit_verdict, audit_report_path, delivery_assessment,
             final_head_sha, final_branch, observed_model, attribution_source, project_id, effective_dir
             FROM tasks WHERE custom_agent_name = ?1
             AND status IN ('done', 'merged', 'failed', 'skipped', 'stopped')
             AND julianday(completed_at) >= julianday(?2)
             AND julianday(completed_at) < julianday(?3)",
        )?;
        let rows = stmt.query_map(
            params![route, start.to_rfc3339(), end.to_rfc3339()],
            super::super::schema::row_to_task,
        )?;
        rows.map(|row| row.map_err(anyhow::Error::from).and_then(|task| task))
            .collect()
    }

    pub(crate) fn overage_task_ids(
        &self,
        range: Option<(DateTime<Local>, DateTime<Local>)>,
    ) -> anyhow::Result<HashSet<String>> {
        let (start, end) = match range {
            Some((start, end)) => (Some(start.to_rfc3339()), Some(end.to_rfc3339())),
            None => (None, None),
        };
        let conn = self.db();
        let mut stmt = conn.prepare(OVERAGE_TASK_IDS_SQL)?;
        let rows = stmt.query_map(params![start, end], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
#[path = "billing_queries_tests.rs"]
mod tests;
