// Task metrics query methods for budget and agent health summaries.
// Exports Store budget usage, success-rate, and cost aggregate queries.
// Deps: Store, rusqlite, chrono, and AgentKind.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, NaiveDate};
use rusqlite::params;

use super::super::Store;
use crate::types::{
    verify_required, AgentKind, DeliveryAssessment, TaskOutcome, TaskStatus, VerifyStatus,
};

pub(crate) const STATS_TASK_QUERY: &str = "SELECT id, created_at, completed_at, repo_path, project_id, tokens FROM tasks WHERE (?1 IS NULL OR created_at >= ?1) AND created_at < ?2 ORDER BY created_at";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatsRow {
    pub id: String,
    pub created_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    pub repo_path: Option<String>,
    pub project_id: Option<String>,
    pub tokens: Option<i64>,
}

impl Store {
    pub fn list_stats_tasks(&self, start: Option<NaiveDate>, end_exclusive: NaiveDate) -> Result<Vec<TaskStatsRow>> {
        let conn = self.db();
        let mut stmt = conn.prepare(STATS_TASK_QUERY)?;
        let rows = stmt.query_map(
            params![start.map(|date| date.to_string()), end_exclusive.to_string()],
            |row| {
                Ok(TaskStatsRow {
                    id: row.get(0)?,
                    created_at: super::super::schema::parse_dt(&row.get::<_, String>(1)?),
                    completed_at: row
                        .get::<_, Option<String>>(2)?
                        .map(|value| super::super::schema::parse_dt(&value)),
                    repo_path: row.get(3)?,
                    project_id: row.get(4)?,
                    tokens: row.get(5)?,
                })
            },
        )?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn budget_usage_summary(
        &self,
        agent: &str,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        self.budget_usage_summary_for_agent(Some(agent), since)
    }

    pub fn budget_usage_summary_all(
        &self,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        self.budget_usage_summary_for_agent(None, since)
    }

    /// Summarize usage for a project budget by matching the persisted repo_path basename.
    /// Tasks do not store project ids, so this mirrors project display fallback identity.
    pub fn budget_usage_summary_for_project(
        &self,
        project_name: &str,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        let conn = self.db();
        let (task_count, total_tokens, total_cost): (i64, i64, f64) = conn.query_row(
            "SELECT COUNT(*) as task_count,
                    COALESCE(SUM(tokens), 0) as total_tokens,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost
             FROM tasks
             WHERE (
                repo_path = ?1
                OR (
                    repo_path IS NOT NULL
                    AND length(repo_path) > length(?1)
                    AND substr(repo_path, length(repo_path) - length(?1) + 1) = ?1
                    AND substr(repo_path, length(repo_path) - length(?1), 1) = '/'
                )
             ) AND (?2 IS NULL OR created_at >= ?2)",
            params![project_name, since.map(|value| value.to_rfc3339())],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((u32::try_from(task_count)?, total_tokens, total_cost))
    }

    fn budget_usage_summary_for_agent(
        &self,
        agent: Option<&str>,
        since: Option<DateTime<Local>>,
    ) -> Result<(u32, i64, f64)> {
        let conn = self.db();
        let (task_count, total_tokens, total_cost): (i64, i64, f64) = conn.query_row(
            "SELECT COUNT(*) as task_count,
                    COALESCE(SUM(tokens), 0) as total_tokens,
                    COALESCE(SUM(cost_usd), 0.0) as total_cost
             FROM tasks WHERE (?1 IS NULL OR agent = ?1) AND (?2 IS NULL OR created_at >= ?2)",
            params![agent, since.map(|value| value.to_rfc3339())],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((u32::try_from(task_count)?, total_tokens, total_cost))
    }

    pub fn agent_success_rates(&self) -> Result<Vec<(AgentKind, f64, usize)>> {
        self.success_rates(None, 5)
    }

    pub fn agent_success_rates_by_category(&self, category: &str) -> Result<Vec<(AgentKind, f64, usize)>> {
        self.success_rates(Some(category), 5)
    }

    fn success_rates(
        &self,
        category: Option<&str>,
        minimum_tasks: usize,
    ) -> Result<Vec<(AgentKind, f64, usize)>> {
        let mut totals = std::collections::HashMap::<AgentKind, (usize, usize)>::new();
        for (agent, outcome) in self.agent_metric_outcomes(category)? {
            if outcome.is_unverified() {
                continue;
            }
            let entry = totals.entry(agent).or_default();
            entry.0 += usize::from(outcome.is_success());
            entry.1 += 1;
        }
        Ok(totals
            .into_iter()
            .filter(|(_, (_, total))| *total >= minimum_tasks)
            .map(|(agent, (successes, total))| {
                (agent, successes as f64 / total as f64, total)
            })
            .collect())
    }

    fn agent_metric_outcomes(
        &self,
        category: Option<&str>,
    ) -> Result<Vec<(AgentKind, TaskOutcome)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent, status, verify_status, verify, delivery_assessment
             FROM tasks
             WHERE status IN ('done', 'merged', 'failed')
               AND (?1 IS NULL OR category = ?1)",
        )?;
        let rows = stmt.query_map(params![category], |row| {
            let agent_str: String = row.get(0)?;
            let status: String = row.get(1)?;
            let verify_status: String = row.get(2)?;
            let verify: Option<String> = row.get(3)?;
            let delivery: Option<String> = row.get(4)?;
            Ok((agent_str, status, verify_status, verify, delivery))
        })?;
        rows.map(|row| {
            let (agent, status, verify_status, verify, delivery) = row?;
            let status = TaskStatus::parse_str(&status)
                .ok_or_else(|| anyhow!("unknown task status in metrics: {status}"))?;
            let verify_status = VerifyStatus::parse_str(&verify_status)
                .ok_or_else(|| anyhow!("unknown verify status in metrics: {verify_status}"))?;
            let agent = AgentKind::parse_str(&agent).unwrap_or(AgentKind::Custom);
            let delivery = delivery
                .as_deref()
                .and_then(DeliveryAssessment::parse_str);
            let outcome = TaskOutcome::derive(
                status,
                verify_status,
                verify_required(verify.as_deref()),
            )
            .with_delivery_assessment(delivery);
            Ok((agent, outcome))
        })
        .collect()
    }

    pub fn agent_avg_costs(&self) -> Result<Vec<(AgentKind, f64)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent, AVG(cost_usd) as avg_cost
             FROM tasks
             WHERE cost_usd IS NOT NULL AND cost_usd > 0
             GROUP BY agent
             HAVING COUNT(*) >= 3",
        )?;
        let rows = stmt.query_map([], |row| {
            let agent_str: String = row.get(0)?;
            let avg_cost: f64 = row.get(1)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            Ok((agent, avg_cost))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }

    pub fn agent_avg_durations(&self) -> Result<Vec<(AgentKind, i64)>> {
        let conn = self.db();
        let mut stmt = conn.prepare(
            "SELECT agent, AVG(duration_ms) / 1000.0 as avg_duration_secs
             FROM tasks
             WHERE duration_ms IS NOT NULL AND duration_ms > 0
             GROUP BY agent
             HAVING COUNT(*) >= 3",
        )?;
        let rows = stmt.query_map([], |row| {
            let agent_str: String = row.get(0)?;
            let duration: f64 = row.get(1)?;
            let agent = AgentKind::parse_str(&agent_str).unwrap_or(AgentKind::Custom);
            Ok((agent, duration.round() as i64))
        })?;
        rows.map(|row| Ok(row?)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::TimeZone;

    fn task(id: &str, created_at: DateTime<Local>, prompt: &str) -> Task {
        Task {
            id: TaskId(id.to_string()), agent: AgentKind::Codex, custom_agent_name: None,
            prompt: prompt.to_string(), resolved_prompt: None, category: None,
            status: TaskStatus::Done, parent_task_id: None, workgroup_id: None,
            caller_kind: None, caller_session_id: None, agent_session_id: None, project_id: None,
            repo_path: Some("/repo".to_string()), worktree_path: None, effective_dir: None, worktree_branch: None,
            final_head_sha: None, final_branch: None, start_sha: None, log_path: None,
            output_path: None, tokens: Some(42), prompt_tokens: None, duration_ms: None,
            requested_model: None, observed_model: None, attribution_source: None,
            cost_usd: None, exit_code: Some(0), created_at, completed_at: None,
            verify: None, verify_status: VerifyStatus::Skipped, pending_reason: None,
            read_only: false, budget: false, audit_verdict: None, audit_report_path: None,
            delivery_assessment: None,
        }
    }

    #[test]
    fn stats_query_selects_only_literal_aggregation_columns() {
        assert_eq!(
            STATS_TASK_QUERY,
            "SELECT id, created_at, completed_at, repo_path, project_id, tokens FROM tasks WHERE (?1 IS NULL OR created_at >= ?1) AND created_at < ?2 ORDER BY created_at",
        );
        assert!(!STATS_TASK_QUERY.contains("prompt"));
    }

    #[test]
    fn stats_query_returns_only_rows_inside_literal_date_range() {
        let store = Store::open_memory().unwrap();
        let at = |value: &str| Local.datetime_from_str(value, "%Y-%m-%d %H:%M:%S").unwrap();
        store.insert_task(&task("t-before", at("2026-08-05 23:59:59"), &"old".repeat(10_000))).unwrap();
        store.insert_task(&task("t-in", at("2026-08-06 00:00:00"), "in range")).unwrap();
        store.insert_task(&task("t-after", at("2026-08-07 00:00:00"), "outside")).unwrap();

        let rows = store
            .list_stats_tasks(Some(NaiveDate::from_ymd_opt(2026, 8, 6).unwrap()), NaiveDate::from_ymd_opt(2026, 8, 7).unwrap())
            .unwrap();

        assert_eq!(rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), vec!["t-in"]);
        assert_eq!(rows[0].repo_path.as_deref(), Some("/repo"));
        assert_eq!(rows[0].project_id, None);
        assert_eq!(rows[0].tokens, Some(42));
    }
}
